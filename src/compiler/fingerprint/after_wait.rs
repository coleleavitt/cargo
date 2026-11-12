//! Checks a unit's freshness again after waiting for another cargo, see [`fresh_after_wait`].

use std::path::PathBuf;
use std::sync::Arc;

use cargo_util::collections::{HashMap, HashSet};
use cargo_util::paths;
use filetime::FileTime;

use crate::compiler::{BuildRunner, Unit, custom_build};
use crate::util;
use crate::util::errors::CargoResult;

use super::{Checksum, Fingerprint, FingerprintComparison, calculate, compare_old_fingerprint};

/// Fingerprints computed by [`fresh_after_wait`], kept for later calls.
///
/// They are separate from [`BuildRunner::fingerprints`], which hold the state at planning and are
/// shared with jobs that write them out.
#[derive(Default)]
pub struct RecheckCache {
    fingerprints: HashMap<Unit, Arc<Fingerprint>>,
    mtime_cache: HashMap<PathBuf, FileTime>,
    checksum_cache: HashMap<PathBuf, Checksum>,
}

/// Whether `unit`, which [`super::prepare_target`] found dirty, is fresh now: another cargo that
/// shares the build directory built it as this build would have, while this build waited for the
/// unit's lock. Call it from the main thread once the unit's dependencies are done.
///
/// This computes the unit's fingerprint again, with the filesystem as it is now, like planning
/// would. The planning fingerprints of the unit's dependencies are out of date at this point:
/// some were built since, by this build or another one.
pub fn fresh_after_wait(
    build_runner: &mut BuildRunner<'_, '_>,
    unit: &Unit,
    cache: &mut RecheckCache,
) -> CargoResult<bool> {
    let known: HashSet<Unit> = cache.fingerprints.keys().cloned().collect();
    reload_build_script_deps(build_runner, unit, &known, &mut HashSet::default());
    let swap = |build_runner: &mut BuildRunner<'_, '_>, cache: &mut RecheckCache| {
        std::mem::swap(&mut build_runner.fingerprints, &mut cache.fingerprints);
        std::mem::swap(&mut build_runner.mtime_cache, &mut cache.mtime_cache);
        std::mem::swap(&mut build_runner.checksum_cache, &mut cache.checksum_cache);
    };
    swap(build_runner, cache);
    let fingerprint = calculate(build_runner, unit);
    swap(build_runner, cache);
    let fingerprint = fingerprint?;

    let loc = build_runner.files().fingerprint_file_path(unit, "");
    let mtime_on_use = build_runner.bcx.gctx.cli_unstable().mtime_on_use;
    let fresh = matches!(
        compare_old_fingerprint(unit, &loc, &fingerprint, mtime_on_use, false),
        FingerprintComparison::Fresh
    );
    if fresh {
        // Dependents hash the fingerprint that planning made. The `local` part of a build script
        // run's comes from the script's output, which the other cargo just wrote.
        let planned = &build_runner.fingerprints[unit];
        *planned.local.lock().unwrap() = fingerprint.local.lock().unwrap().clone();
        planned.clear_memoized();
    }

    // Keep the fingerprints of units that are built, as they stay so for the rest of the build:
    // this build holds them shared. A dependency that this build still compiles has no
    // fingerprint on disk yet, and is computed again next time.
    cache.fingerprints.retain(|unit, fingerprint| {
        known.contains(unit)
            || (fingerprint.fs_status.up_to_date()
                && paths::read(&build_runner.files().fingerprint_file_path(unit, ""))
                    .is_ok_and(|hash| hash == util::to_hex(fingerprint.hash_u64())))
    });
    Ok(fresh)
}

/// Reloads what the build scripts that `unit` depends on, and that are not in `known`, declared
/// in their latest output: planning read their outputs from before this build, and a build script
/// run's fingerprint depends on them.
fn reload_build_script_deps(
    build_runner: &mut BuildRunner<'_, '_>,
    unit: &Unit,
    known: &HashSet<Unit>,
    seen: &mut HashSet<Unit>,
) {
    if known.contains(unit) || !seen.insert(unit.clone()) {
        return;
    }
    if unit.mode.is_run_custom_build() {
        custom_build::parse_previous_explicit_deps(build_runner, unit);
    }
    for dep in Vec::from(build_runner.unit_deps(unit)) {
        reload_build_script_deps(build_runner, &dep.unit, known, seen);
    }
}
