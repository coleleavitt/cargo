#!/usr/bin/env bash
# build-patched-cargo.sh
# -----------------------------------------------------------------------------
# Build cargo from pristine upstream rust-lang/cargo @ the exact commit that
# the active rustup nightly toolchain was built from, apply our local patch
# series (patches/*.patch), and install the result into the nightly toolchain
# so cargo and rustc always share the same compiler/ABI.
#
# Why this exists: a standalone `cargo install --path .` binary pins an old
# release-channel cargo that drives a newer nightly rustc, which breaks
# -Zcheck-cfg and produces ABI-incompatible artifacts. The right place for a
# patched cargo is *inside* the rustup toolchain directory.
#
# This script is designed to be run from `_rustup_daily_update` (in .bashrc)
# every time the nightly toolchain commit moves, AND is safe to run manually.
#
# State:
#   $STATE_DIR/installed-upstream-commit   sha1 of upstream rust-lang/cargo
#                                          the currently-installed patched
#                                          cargo was built from
#   $STATE_DIR/installed-patched-commit    short sha of the patched cargo
#                                          we wrote (so we can detect when
#                                          rustup has overwritten our binary
#                                          with a new pristine one)
#   $STATE_DIR/last-build.log              last build's full log
#   $TOOLCHAIN_BIN/cargo.bak               pristine rustup-shipped cargo,
#                                          refreshed whenever rustup ships a
#                                          new upstream commit
#
# Usage:
#   scripts/build-patched-cargo.sh                # build + install if needed
#   scripts/build-patched-cargo.sh --force        # rebuild even if up to date
#   scripts/build-patched-cargo.sh --check        # exit 0=up-to-date, 1=stale
#   scripts/build-patched-cargo.sh --no-install   # build + verify only
#   scripts/build-patched-cargo.sh --restore      # restore pristine cargo
#   scripts/build-patched-cargo.sh --status       # report current state
# -----------------------------------------------------------------------------
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PATCH_DIR="$REPO_ROOT/patches"
TOOLCHAIN="${RUSTUP_PATCHED_TOOLCHAIN:-nightly-x86_64-unknown-linux-gnu}"
TOOLCHAIN_BIN="$HOME/.rustup/toolchains/$TOOLCHAIN/bin"
STATE_DIR="$HOME/.cache/cargo-patched"
STATE_FILE="$STATE_DIR/installed-upstream-commit"
PATCHED_COMMIT_FILE="$STATE_DIR/installed-patched-commit"
LOG_FILE="$STATE_DIR/last-build.log"
WORK_BRANCH="build/patched-nightly"

mkdir -p "$STATE_DIR"

FORCE=0
INSTALL=1
CHECK_ONLY=0
RESTORE=0
STATUS=0
QUIET="${RUSTUP_PATCHED_QUIET:-0}"

for arg in "$@"; do
    case "$arg" in
        --force)      FORCE=1 ;;
        --no-install) INSTALL=0 ;;
        --check)      CHECK_ONLY=1 ;;
        --restore)    RESTORE=1 ;;
        --status)     STATUS=1 ;;
        --quiet|-q)   QUIET=1 ;;
        -h|--help)
            sed -n '2,30p' "$0"
            exit 0 ;;
        *)
            echo "unknown arg: $arg" >&2; exit 2 ;;
    esac
done

log() { [[ "$QUIET" == 1 ]] || printf '\033[1;34m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mwarn:\033[0m %s\n' "$*" >&2; }
err()  { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; }

# ── helpers ─────────────────────────────────────────────────────────────────
refresh_backup_if_rustup_updated() {
    # rustup update overwrites $TOOLCHAIN_BIN/cargo with a fresh pristine
    # binary, blowing away our patched install. Detect that case by comparing
    # the live binary's commit against the patched commit we recorded. If they
    # differ, rustup has shipped a new cargo — promote it to the new backup so
    # we rebase our patches onto the *new* upstream commit, not the stale one.
    [[ -x "$TOOLCHAIN_BIN/cargo" ]] || return 0
    local live_commit patched_commit
    live_commit="$("$TOOLCHAIN_BIN/cargo" --version 2>/dev/null | parse_commit)"
    patched_commit=""
    [[ -f "$PATCHED_COMMIT_FILE" ]] && patched_commit="$(cat "$PATCHED_COMMIT_FILE")"

    if [[ -n "$patched_commit" && "$live_commit" != "$patched_commit" ]]; then
        # The live binary is NOT our patched cargo => rustup replaced it.
        log "rustup shipped a new cargo ($live_commit); refreshing pristine backup"
        cp "$TOOLCHAIN_BIN/cargo" "$TOOLCHAIN_BIN/cargo.bak"
        rm -f "$STATE_FILE" "$PATCHED_COMMIT_FILE"
    fi
}

get_pristine_cargo() {
    # Returns the path to the rustup-shipped (unpatched) cargo. After
    # refresh_backup_if_rustup_updated(), cargo.bak always holds the pristine
    # binary matching the active toolchain.
    if [[ -x "$TOOLCHAIN_BIN/cargo.bak" ]]; then
        echo "$TOOLCHAIN_BIN/cargo.bak"
    elif [[ -x "$TOOLCHAIN_BIN/cargo" ]]; then
        echo "$TOOLCHAIN_BIN/cargo"
    else
        return 1
    fi
}

parse_commit() {
    # Extract the short sha from a `cargo --version` string. Returns "" on miss.
    sed -n 's/.*(\([0-9a-f]\{7,\}\) .*/\1/p'
}

resolve_upstream_commit() {
    # Resolve a short sha to a full sha by querying git in the fork.
    cd "$REPO_ROOT"
    git rev-parse --verify --quiet "$1^{commit}" 2>/dev/null
}

# ── --restore: put the pristine nightly cargo back ──────────────────────────
if [[ "$RESTORE" == 1 ]]; then
    if [[ -f "$TOOLCHAIN_BIN/cargo.bak" ]]; then
        cp "$TOOLCHAIN_BIN/cargo.bak" "$TOOLCHAIN_BIN/cargo"
        rm -f "$STATE_FILE" "$PATCHED_COMMIT_FILE"
        log "restored pristine cargo from $TOOLCHAIN_BIN/cargo.bak"
        "$TOOLCHAIN_BIN/cargo" --version
    else
        err "no backup at $TOOLCHAIN_BIN/cargo.bak — nothing to restore"
        exit 1
    fi
    exit 0
fi

# ── --status: report what's installed and whether it's current ──────────────
if [[ "$STATUS" == 1 ]]; then
    printf 'patched cargo install: '
    if [[ -f "$STATE_FILE" ]]; then
        printf 'patched (built from upstream %s)\n' "$(cat "$STATE_FILE")"
    else
        printf 'pristine (no patches applied)\n'
    fi
    printf 'rustup nightly cargo:  %s\n' "$("$TOOLCHAIN_BIN/cargo" --version)"
    if [[ -x "$TOOLCHAIN_BIN/cargo.bak" ]]; then
        printf 'backed-up cargo:       %s\n' "$("$TOOLCHAIN_BIN/cargo.bak" --version)"
    fi
    exit 0
fi

# ── 1. Figure out which upstream commit the active nightly cargo came from ──
refresh_backup_if_rustup_updated
PRISTINE_CARGO="$(get_pristine_cargo)" || { err "no cargo binary in $TOOLCHAIN_BIN"; exit 1; }
NIGHTLY_VERSION="$("$PRISTINE_CARGO" --version)"
SHORT_COMMIT="$(printf '%s' "$NIGHTLY_VERSION" | parse_commit)"
if [[ -z "$SHORT_COMMIT" ]]; then
    err "could not parse upstream commit from: $NIGHTLY_VERSION"
    exit 1
fi
log "pristine cargo: $NIGHTLY_VERSION (commit $SHORT_COMMIT)"

# ── 2. Idempotency check: skip if already up to date ────────────────────────
CURRENT_INSTALLED=""
if [[ -f "$STATE_FILE" ]]; then
    CURRENT_INSTALLED="$(cat "$STATE_FILE")"
fi

if [[ "$CHECK_ONLY" == 1 ]]; then
    if [[ -z "$CURRENT_INSTALLED" || "$CURRENT_INSTALLED" != "$SHORT_COMMIT"* ]]; then
        log "stale: installed=$CURRENT_INSTALLED, target=$SHORT_COMMIT"
        exit 1
    fi
    log "up to date: $CURRENT_INSTALLED"
    exit 0
fi

if [[ "$FORCE" != 1 && -n "$CURRENT_INSTALLED" && "$CURRENT_INSTALLED" == "$SHORT_COMMIT"* ]]; then
    log "patched cargo is up to date (upstream $SHORT_COMMIT); use --force to rebuild"
    exit 0
fi

# ── 3. Snapshot the patch series BEFORE we touch git state ──────────────────
# If patches/ is ever tracked on another branch, a checkout could swap it out
# from under us. Snapshot to a tmpdir so the apply step is immune to that.
shopt -s nullglob
PATCHES_SRC=("$PATCH_DIR"/*.patch)
shopt -u nullglob
if [[ ${#PATCHES_SRC[@]} -eq 0 ]]; then
    err "no patches found in $PATCH_DIR"
    exit 1
fi
PATCH_SNAPSHOT="$(mktemp -d)"
trap 'rm -rf "$PATCH_SNAPSHOT"' EXIT
cp "${PATCHES_SRC[@]}" "$PATCH_SNAPSHOT/"
PATCHES=("$PATCH_SNAPSHOT"/*.patch)

# ── 4. Fetch upstream and check out the pristine commit ─────────────────────
cd "$REPO_ROOT"

if ! git rev-parse --verify --quiet "$SHORT_COMMIT^{commit}" >/dev/null 2>&1; then
    log "commit not present locally; fetching origin master…"
    git fetch origin master --tags --quiet
fi
FULL_COMMIT="$(resolve_upstream_commit "$SHORT_COMMIT" || true)"
if [[ -z "$FULL_COMMIT" ]]; then
    err "upstream commit $SHORT_COMMIT not found even after fetch."
    err "your nightly may be ahead of origin/master; run 'git fetch origin'"
    exit 1
fi

# Bail if the working tree has uncommitted changes — git checkout would clobber them.
if ! git diff --quiet || ! git diff --cached --quiet; then
    err "working tree is dirty in $REPO_ROOT; commit or stash before building"
    exit 1
fi

log "checking out pristine upstream @ $FULL_COMMIT on $WORK_BRANCH"
git checkout -B "$WORK_BRANCH" "$FULL_COMMIT" >/dev/null 2>&1

# ── 5. Apply the patch series ───────────────────────────────────────────────
log "applying ${#PATCHES[@]} patch(es)…"
if ! git am --quiet "${PATCHES[@]}" 2>>"$LOG_FILE"; then
    err "git am failed — patches need a refresh against upstream $SHORT_COMMIT"
    err "see $LOG_FILE for details"
    git am --abort >/dev/null 2>&1 || true
    exit 1
fi
log "patches applied:"
git log --oneline "$FULL_COMMIT..HEAD" | sed 's/^/    /'

# ── 6. Build with the nightly toolchain + matching release channel ──────────
# CFG_RELEASE_CHANNEL=nightly makes the built cargo identify itself as nightly
# so the rustc it drives accepts -Z flags (e.g. -Zcheck-cfg) the same way the
# original rustup-shipped cargo did.
log "building cargo (release) with $TOOLCHAIN…"
: > "$LOG_FILE"
if ! RUSTC_WRAPPER="" CFG_RELEASE_CHANNEL=nightly \
        rustup run "$TOOLCHAIN" cargo build --release -p cargo >>"$LOG_FILE" 2>&1; then
    err "build failed — see $LOG_FILE"
    exit 1
fi

BUILT="$REPO_ROOT/target/release/cargo"
[[ -x "$BUILT" ]] || { err "build produced no binary at $BUILT"; exit 1; }
log "built: $("$BUILT" --version)"

# ── 7. Install into the nightly toolchain (with backup) ─────────────────────
if [[ "$INSTALL" == 1 ]]; then
    if [[ ! -f "$TOOLCHAIN_BIN/cargo.bak" ]]; then
        # Only back up if the current binary is *not* already patched. If we've
        # been here before, cargo.bak should exist; if it doesn't, the current
        # binary is the pristine one we want to preserve.
        cp "$TOOLCHAIN_BIN/cargo" "$TOOLCHAIN_BIN/cargo.bak"
        log "backed up pristine cargo -> $TOOLCHAIN_BIN/cargo.bak"
    fi
    cp "$BUILT" "$TOOLCHAIN_BIN/cargo"

    # Ensure ~/.cargo/bin/cargo is the rustup shim, not a stale standalone binary
    # (a prior `cargo install --path . --force` could have replaced it).
    if [[ -e "$HOME/.cargo/bin/cargo" && ! -L "$HOME/.cargo/bin/cargo" ]]; then
        log "replacing standalone ~/.cargo/bin/cargo with rustup shim"
        ln -sf rustup "$HOME/.cargo/bin/cargo"
    fi

    # Record the upstream base commit (idempotency) and our patched binary's
    # own commit (so refresh_backup_if_rustup_updated can tell when rustup has
    # later overwritten our binary with a fresh pristine one).
    echo "$FULL_COMMIT" > "$STATE_FILE"
    "$BUILT" --version | parse_commit > "$PATCHED_COMMIT_FILE"

    log "installed patched cargo into $TOOLCHAIN"
    log "verify: $(cargo --version)  /  $(rustc --version)"
else
    log "skipped install (--no-install); binary at $BUILT"
fi

log "done."
