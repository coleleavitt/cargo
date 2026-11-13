//! A per-user cache of compiled units from immutable packages.
//!
//! Units of registry and git packages that only depend on other such units
//! are copied from the build-dir into the cache after they are built, and
//! copied back into another build-dir that lacks them before freshness is
//! checked. Each build-dir stays private to its workspace, so separate
//! worktrees share no locks, while dependencies are compiled once per user.
//!
//! An entry is the unit's whole `build/<pkg>/<hash>` directory, so its
//! fingerprint, dep-info and build script output come with it, mtimes
//! included. Restoring only fills in a missing unit directory: whether the
//! unit is fresh is still decided by the fingerprint check.
//!
//! An entry is keyed by its path relative to the build-dir root (which holds
//! the target, the profile, the package name and the unit hash), the full
//! `rustc -vV`, the commit of a git package, and the keys of its
//! dependencies. Entries are immutable: they are published by renaming a
//! complete temporary directory into place, and the first one wins.
//!
//! Two builds of the same key are not always identical, for example when a
//! build script generates code with absolute paths into its `OUT_DIR`. A unit
//! must therefore only be restored on top of the same builds of its
//! dependencies that it was compiled against. Each unit directory carries a
//! random build id, and an entry records the ids of the dependencies it was
//! built with.
//!
//! The cache is bounded by `build.unit-cache-max-size`. Each entry has a
//! marker file whose mtime is the last time a build restored or published
//! it, and a build that added entries removes the least recently used ones
//! while the cache is over the limit.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use cargo_util::collections::HashMap;
use cargo_util::paths;
use filetime::FileTime;
use tracing::debug;

use crate::compiler::job_queue::{Job, Work};
use crate::compiler::{BuildRunner, CompileMode, Unit};
use crate::context::{ConfigRelativePathResolveExt, SizeConfig};
use crate::util::errors::CargoResult;
use crate::util::short_hash;
use crate::workspace::gc::parse_human_size;

/// File in a unit directory holding the id of the build that produced it.
const ID_FILE: &str = "unit-cache-id";
/// File in a cache entry listing `<dep key> <dep id>` per dependency.
const DEPS_FILE: &str = "unit-cache-deps";
/// The `-Zfine-grain-locking` lock file, which belongs to one build-dir.
const LOCK_FILE: &str = ".lock";
/// File in a cache entry whose mtime is the entry's last use.
const USED_FILE: &str = "unit-cache-used";
/// File in a cache entry holding the entry's size in bytes.
const SIZE_FILE: &str = "unit-cache-size";
/// Prefix of an entry that is being removed.
const EVICT_PREFIX: &str = ".evict-";
const DEFAULT_MAX_SIZE: u64 = 50 << 30;

pub struct UnitCache {
    units: HashMap<Unit, Arc<CachedUnit>>,
    root: PathBuf,
    max_size: u64,
    /// Whether this build added an entry.
    published: Arc<AtomicBool>,
}

struct CachedUnit {
    key: String,
    /// The unit's directory in this build-dir.
    dir: PathBuf,
    /// The unit's entry in the cache.
    entry: PathBuf,
    deps: Vec<Arc<CachedUnit>>,
}

impl UnitCache {
    /// Returns the cache if it is enabled for this build, and restores every
    /// cacheable unit that is missing from the build-dir and present in the
    /// cache.
    pub fn restore(build_runner: &BuildRunner<'_, '_>) -> CargoResult<Option<UnitCache>> {
        let gctx = build_runner.bcx.gctx;
        let enabled = gctx.cli_unstable().unit_cache
            || gctx.get_env("CARGO_UNIT_CACHE").is_ok_and(|v| v == "1");
        if !enabled || !gctx.cli_unstable().build_dir_new_layout {
            return Ok(None);
        }
        let build_config = gctx.build_config()?;
        let root = match &build_config.unit_cache_dir {
            Some(dir) => dir.resolve_path(gctx),
            None => gctx.home().as_path_unlocked().join("unit-cache"),
        };
        let max_size = match &build_config.unit_cache_max_size {
            None => DEFAULT_MAX_SIZE,
            Some(SizeConfig::Bytes(n)) => *n,
            Some(SizeConfig::String(s)) => {
                parse_human_size(s).map_err(|e| e.context("invalid `build.unit-cache-max-size`"))?
            }
        };
        let mut cache = UnitCache {
            units: HashMap::default(),
            root: root.clone(),
            max_size,
            published: Arc::default(),
        };
        let mut keys = KeyCx {
            build_runner,
            root: &root,
            build_root: build_runner.bcx.ws.build_dir().into_path_unlocked(),
            rustc: short_hash(&build_runner.bcx.rustc().verbose_version),
            memo: HashMap::default(),
        };
        for unit in build_runner.bcx.unit_graph.keys() {
            if let Some(c) = keys.cached_unit(unit) {
                cache.units.insert(unit.clone(), c);
            }
        }

        let mut ids = HashMap::default();
        let mut restored = 0;
        for c in cache.units.values() {
            current_id(c, &mut ids, &mut restored);
        }
        if restored > 0 {
            gctx.shell().verbose(|shell| {
                shell.status(
                    "Restored",
                    format!("{restored} units from {}", root.display()),
                )
            })?;
        }
        Ok(Some(cache))
    }

    /// Gives a cacheable unit a new id if it is about to be built, and
    /// publishes it once its job has succeeded.
    pub fn prepare_job(&self, unit: &Unit, job: &mut Job) -> CargoResult<()> {
        let Some(c) = self.units.get(unit) else {
            return Ok(());
        };
        if job.freshness().is_dirty() || read_id(&c.dir).is_none() {
            paths::write(c.dir.join(ID_FILE), new_id())?;
        }
        let c = Arc::clone(c);
        let published = Arc::clone(&self.published);
        job.after(Work::new(move |_| {
            match publish(&c) {
                Ok(true) => published.store(true, Ordering::Relaxed),
                Ok(false) => {}
                Err(e) => debug!(
                    "failed to publish {} to the unit cache: {e}",
                    c.dir.display()
                ),
            }
            Ok(())
        }));
        Ok(())
    }

    /// Removes the least recently used entries while the cache is larger
    /// than its maximum size. Only a build that added entries can have made
    /// it larger.
    pub fn evict(&self) {
        if !self.published.load(Ordering::Relaxed) {
            return;
        }
        if let Err(e) = evict(&self.root, self.max_size) {
            debug!("failed to evict from {}: {e}", self.root.display());
        }
    }
}

struct KeyCx<'a, 'b, 'gctx> {
    build_runner: &'a BuildRunner<'b, 'gctx>,
    root: &'a Path,
    build_root: PathBuf,
    rustc: String,
    memo: HashMap<Unit, Option<Arc<CachedUnit>>>,
}

impl KeyCx<'_, '_, '_> {
    /// Returns the cache entry of `unit`, or `None` if the unit or one of its
    /// dependencies is not cacheable.
    fn cached_unit(&mut self, unit: &Unit) -> Option<Arc<CachedUnit>> {
        if let Some(c) = self.memo.get(unit) {
            return c.clone();
        }
        let c = self.compute(unit);
        self.memo.insert(unit.clone(), c.clone());
        c
    }

    fn compute(&mut self, unit: &Unit) -> Option<Arc<CachedUnit>> {
        let source = unit.pkg.package_id().source_id();
        let mode_ok = matches!(
            unit.mode,
            CompileMode::Build | CompileMode::Check { test: false } | CompileMode::RunCustomBuild
        );
        if !mode_ok || unit.is_std || !(source.is_registry() || source.is_git()) {
            return None;
        }
        let mut deps = Vec::new();
        for dep in self.build_runner.unit_deps(unit) {
            deps.push(self.cached_unit(&dep.unit)?);
        }
        deps.sort_by(|a, b| a.key.cmp(&b.key));
        deps.dedup_by(|a, b| a.key == b.key);

        let dir = self.build_runner.files().build_unit_dir(unit);
        let rel = dir.strip_prefix(&self.build_root).ok()?;
        let dep_keys: Vec<&str> = deps.iter().map(|d| d.key.as_str()).collect();
        let key = short_hash(&(&self.rustc, rel, source.precise_git_fragment(), dep_keys));
        let entry = self.root.join(unit.pkg.name().as_str()).join(format!(
            "{}-{key}",
            self.build_runner.files().unit_hash(unit)
        ));
        Some(Arc::new(CachedUnit {
            key,
            dir,
            entry,
            deps,
        }))
    }
}

/// Returns the id of the unit's build in the build-dir, restoring it from
/// the cache first if it is missing.
fn current_id(
    c: &CachedUnit,
    ids: &mut HashMap<String, Option<String>>,
    restored: &mut usize,
) -> Option<String> {
    if let Some(id) = ids.get(&c.key) {
        return id.clone();
    }
    for dep in &c.deps {
        current_id(dep, ids, restored);
    }
    let id = if c.dir.exists() {
        read_id(&c.dir)
    } else if c.entry.exists() {
        match restore_entry(c, ids) {
            Ok(true) => {
                *restored += 1;
                touch(&c.entry);
                read_id(&c.dir)
            }
            Ok(false) => None,
            Err(e) => {
                debug!("failed to restore {}: {e}", c.entry.display());
                None
            }
        }
    } else {
        None
    };
    ids.insert(c.key.clone(), id.clone());
    id
}

/// Whether the copy of an entry in `dir` was built against the dependency
/// builds now in the build-dir.
fn deps_match(c: &CachedUnit, dir: &Path, ids: &HashMap<String, Option<String>>) -> bool {
    let Ok(recorded) = fs::read_to_string(dir.join(DEPS_FILE)) else {
        return false;
    };
    let recorded: HashMap<&str, &str> = recorded
        .lines()
        .filter_map(|line| line.split_once(' '))
        .collect();
    c.deps.iter().all(|dep| {
        let current = ids.get(&dep.key).cloned().flatten();
        current.is_some() && recorded.get(dep.key.as_str()).copied() == current.as_deref()
    })
}

/// Copies the entry into the build-dir if it was built against the
/// dependency builds there. Returns whether it did.
fn restore_entry(c: &CachedUnit, ids: &HashMap<String, Option<String>>) -> io::Result<bool> {
    let parent = c.dir.parent().unwrap();
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(".unit-cache-{}-{}", std::process::id(), new_id()));
    // The entry can be evicted, and published again by another build, while
    // it is copied. The copy is only complete if the entry still has the id
    // of the copy once the copy has been made; see `evict`.
    let result = copy_dir(&c.entry, &tmp).and_then(|_| {
        let id = read_id(&tmp);
        if id.is_some() && id == read_id(&c.entry) && deps_match(c, &tmp, ids) {
            fs::rename(&tmp, &c.dir).map(|()| true)
        } else {
            Ok(false)
        }
    });
    if !matches!(result, Ok(true)) {
        let _ = fs::remove_dir_all(&tmp);
    }
    result
}

/// Copies the unit into the cache unless it is there already. Returns
/// whether it did.
fn publish(c: &CachedUnit) -> io::Result<bool> {
    if c.entry.exists() {
        touch(&c.entry);
        return Ok(false);
    }
    let Some(id) = read_id(&c.dir) else {
        return Ok(false);
    };
    let mut deps = String::new();
    for dep in &c.deps {
        let Some(dep_id) = read_id(&dep.dir) else {
            return Ok(false);
        };
        // Built against another build of the dependency than the cache
        // holds, so this entry could never be restored.
        if read_id(&dep.entry).is_some_and(|cached| cached != dep_id) {
            return Ok(false);
        }
        deps.push_str(&format!("{} {dep_id}\n", dep.key));
    }
    let parent = c.entry.parent().unwrap();
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(".tmp-{}-{}", std::process::id(), new_id()));
    let result = copy_dir(&c.dir, &tmp).and_then(|size| {
        fs::write(tmp.join(DEPS_FILE), &deps)?;
        fs::write(tmp.join(ID_FILE), &id)?;
        fs::write(tmp.join(USED_FILE), "")?;
        let size = size + (deps.len() + id.len()) as u64;
        fs::write(tmp.join(SIZE_FILE), size.to_string())
    });
    let result = match result.and_then(|()| fs::rename(&tmp, &c.entry)) {
        Ok(()) => return Ok(true),
        // Another build published this entry first.
        Err(_) if c.entry.exists() => Ok(false),
        Err(e) => Err(e),
    };
    let _ = fs::remove_dir_all(&tmp);
    result
}

/// Removes the least recently used entries until the cache is at most
/// `max_size` bytes.
///
/// An entry is first renamed out of the way and only then removed. A
/// restore reads the entry by path and checks afterwards that the entry
/// still has the id it copied (`restore_entry`). If the rename happens
/// before that check, the check fails and the copy is discarded. If it
/// happens after, every file was copied while the entry was complete:
/// files are only deleted once renamed, and a copied or reflinked file keeps
/// its data when the original is deleted. Deleting in place would instead
/// let a restore that is walking the entry miss files that were already
/// deleted, and leave an incomplete unit in its build-dir. It would also
/// leave an incomplete entry in the cache if the removal were interrupted.
fn evict(root: &Path, max_size: u64) -> io::Result<()> {
    let mut entries = Vec::new();
    let mut total = 0;
    for pkg in fs::read_dir(root)? {
        for entry in fs::read_dir(pkg?.path())? {
            let path = entry?.path();
            let name = path.file_name().unwrap().to_string_lossy();
            if name.starts_with(EVICT_PREFIX) {
                // Left behind by an interrupted eviction; nothing reads it.
                let _ = fs::remove_dir_all(&path);
                continue;
            }
            if name.starts_with('.') {
                continue;
            }
            // Gone if another build evicted it in the meantime.
            let Ok(used) = fs::metadata(path.join(USED_FILE)).and_then(|m| m.modified()) else {
                continue;
            };
            let size = match fs::read_to_string(path.join(SIZE_FILE)) {
                Ok(s) => s.trim().parse().unwrap_or(0),
                Err(_) => continue,
            };
            total += size;
            entries.push((used, size, path));
        }
    }
    entries.sort();
    for (_, size, path) in entries {
        if total <= max_size {
            break;
        }
        let tmp = path.with_file_name(format!("{EVICT_PREFIX}{}-{}", std::process::id(), new_id()));
        if fs::rename(&path, &tmp).is_ok() {
            fs::remove_dir_all(&tmp)?;
            total -= size;
        }
    }
    Ok(())
}

/// Records that the entry was used now.
fn touch(entry: &Path) {
    let result = fs::OpenOptions::new()
        .append(true)
        .open(entry.join(USED_FILE))
        .and_then(|f| f.set_modified(SystemTime::now()));
    if let Err(e) = result {
        debug!("failed to mark {} as used: {e}", entry.display());
    }
}

fn read_id(dir: &Path) -> Option<String> {
    fs::read_to_string(dir.join(ID_FILE)).ok()
}

fn new_id() -> String {
    format!("{:016x}", rand::random::<u64>())
}

/// Copies a directory tree, keeping permissions and mtimes, which the
/// fingerprint check compares. Returns the size of the copied files.
fn copy_dir(from: &Path, to: &Path) -> io::Result<u64> {
    fs::create_dir(to)?;
    let mut size = 0;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        if [LOCK_FILE, USED_FILE, SIZE_FILE]
            .map(Into::into)
            .contains(&entry.file_name())
        {
            continue;
        }
        let src = entry.path();
        let dst = to.join(entry.file_name());
        let ty = entry.file_type()?;
        if ty.is_dir() {
            size += copy_dir(&src, &dst)?;
        } else if ty.is_symlink() {
            copy_symlink(&src, &dst)?;
        } else {
            size += copy_file(&src, &dst)?;
        }
    }
    let meta = fs::metadata(from)?;
    filetime::set_file_mtime(to, FileTime::from_last_modification_time(&meta))?;
    Ok(size)
}

fn copy_file(src: &Path, dst: &Path) -> io::Result<u64> {
    if reflink(src, dst).is_err() {
        fs::copy(src, dst)?;
    }
    let meta = fs::metadata(src)?;
    fs::set_permissions(dst, meta.permissions())?;
    filetime::set_file_times(
        dst,
        FileTime::from_last_access_time(&meta),
        FileTime::from_last_modification_time(&meta),
    )?;
    Ok(meta.len())
}

#[cfg(unix)]
fn copy_symlink(src: &Path, dst: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(fs::read_link(src)?, dst)
}

#[cfg(not(unix))]
fn copy_symlink(src: &Path, dst: &Path) -> io::Result<()> {
    copy_file(src, dst)
}

/// Shares the data blocks of `src` with a new file `dst` (XFS, Btrfs).
#[cfg(target_os = "linux")]
fn reflink(src: &Path, dst: &Path) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    const FICLONE: libc::c_ulong = 0x40049409;
    let src_file = fs::File::open(src)?;
    let dst_file = fs::File::create_new(dst)?;
    // SAFETY: both descriptors are open for the duration of the call.
    let ret = unsafe { libc::ioctl(dst_file.as_raw_fd(), FICLONE as _, src_file.as_raw_fd()) };
    if ret == 0 {
        return Ok(());
    }
    let err = io::Error::last_os_error();
    drop(dst_file);
    let _ = fs::remove_file(dst);
    Err(err)
}

#[cfg(not(target_os = "linux"))]
fn reflink(_src: &Path, _dst: &Path) -> io::Result<()> {
    Err(io::ErrorKind::Unsupported.into())
}
