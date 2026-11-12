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

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cargo_util::collections::HashMap;
use cargo_util::paths;
use filetime::FileTime;
use tracing::debug;

use crate::compiler::job_queue::{Job, Work};
use crate::compiler::{BuildRunner, CompileMode, Unit};
use crate::context::ConfigRelativePathResolveExt;
use crate::util::errors::CargoResult;
use crate::util::short_hash;

/// File in a unit directory holding the id of the build that produced it.
const ID_FILE: &str = "unit-cache-id";
/// File in a cache entry listing `<dep key> <dep id>` per dependency.
const DEPS_FILE: &str = "unit-cache-deps";
/// The `-Zfine-grain-locking` lock file, which belongs to one build-dir.
const LOCK_FILE: &str = ".lock";

pub struct UnitCache {
    units: HashMap<Unit, Arc<CachedUnit>>,
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
        let root = match &gctx.build_config()?.unit_cache_dir {
            Some(dir) => dir.resolve_path(gctx),
            None => gctx.home().as_path_unlocked().join("unit-cache"),
        };
        let mut cache = UnitCache {
            units: HashMap::default(),
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
        job.after(Work::new(move |_| {
            if let Err(e) = publish(&c) {
                debug!(
                    "failed to publish {} to the unit cache: {e}",
                    c.dir.display()
                );
            }
            Ok(())
        }));
        Ok(())
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
    } else if deps_match(c, ids) {
        match restore_entry(c) {
            Ok(()) => {
                *restored += 1;
                read_id(&c.dir)
            }
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

/// Whether the entry was built against the dependency builds now in the
/// build-dir.
fn deps_match(c: &CachedUnit, ids: &HashMap<String, Option<String>>) -> bool {
    let Ok(recorded) = fs::read_to_string(c.entry.join(DEPS_FILE)) else {
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

fn restore_entry(c: &CachedUnit) -> io::Result<()> {
    let parent = c.dir.parent().unwrap();
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(".unit-cache-{}-{}", std::process::id(), new_id()));
    let result = copy_dir(&c.entry, &tmp).and_then(|()| fs::rename(&tmp, &c.dir));
    if result.is_err() {
        let _ = fs::remove_dir_all(&tmp);
    }
    result
}

fn publish(c: &CachedUnit) -> io::Result<()> {
    if c.entry.exists() {
        return Ok(());
    }
    let Some(id) = read_id(&c.dir) else {
        return Ok(());
    };
    let mut deps = String::new();
    for dep in &c.deps {
        let Some(dep_id) = read_id(&dep.dir) else {
            return Ok(());
        };
        // Built against another build of the dependency than the cache
        // holds, so this entry could never be restored.
        if read_id(&dep.entry).is_some_and(|cached| cached != dep_id) {
            return Ok(());
        }
        deps.push_str(&format!("{} {dep_id}\n", dep.key));
    }
    let parent = c.entry.parent().unwrap();
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(".tmp-{}-{}", std::process::id(), new_id()));
    let result = copy_dir(&c.dir, &tmp)
        .and_then(|()| fs::write(tmp.join(DEPS_FILE), deps))
        .and_then(|()| fs::write(tmp.join(ID_FILE), id));
    let result = match result.and_then(|()| fs::rename(&tmp, &c.entry)) {
        Ok(()) => return Ok(()),
        // Another build published this entry first.
        Err(_) if c.entry.exists() => Ok(()),
        Err(e) => Err(e),
    };
    let _ = fs::remove_dir_all(&tmp);
    result
}

fn read_id(dir: &Path) -> Option<String> {
    fs::read_to_string(dir.join(ID_FILE)).ok()
}

fn new_id() -> String {
    format!("{:016x}", rand::random::<u64>())
}

/// Copies a directory tree, keeping permissions and mtimes, which the
/// fingerprint check compares.
fn copy_dir(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        if entry.file_name() == LOCK_FILE {
            continue;
        }
        let src = entry.path();
        let dst = to.join(entry.file_name());
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir(&src, &dst)?;
        } else if ty.is_symlink() {
            copy_symlink(&src, &dst)?;
        } else {
            copy_file(&src, &dst)?;
        }
    }
    let meta = fs::metadata(from)?;
    filetime::set_file_mtime(to, FileTime::from_last_modification_time(&meta))
}

fn copy_file(src: &Path, dst: &Path) -> io::Result<()> {
    if reflink(src, dst).is_err() {
        fs::copy(src, dst)?;
    }
    let meta = fs::metadata(src)?;
    fs::set_permissions(dst, meta.permissions())?;
    filetime::set_file_times(
        dst,
        FileTime::from_last_access_time(&meta),
        FileTime::from_last_modification_time(&meta),
    )
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
