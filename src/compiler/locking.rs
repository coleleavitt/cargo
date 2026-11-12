//! This module handles the locking logic during compilation.
//!
//! With `-Zfine-grain-locking`, each build unit has a lock file. A build holds it shared while it
//! uses the unit, and exclusive while it compiles the unit. Several cargo processes may share a
//! build directory, so the protocol must not let them wait for each other in a cycle:
//!
//! - A build never blocks on a unit's exclusive lock. It only tries to take it. When that fails,
//!   it blocks on the shared lock instead, and checks the unit's fingerprint again once it has it.
//! - A build that holds a unit's exclusive lock waits for no other unit lock. It only compiles the
//!   unit and then downgrades the lock to shared.
//!
//! So a blocked build always waits for a build that compiles, and that build finishes the unit
//! without waiting for anyone. When the fingerprint check after the wait shows that the other build
//! made the unit as this build would have, this build uses it instead of compiling it again.
//!
//! A unit can stay dirty for this build after the wait. Then another build uses it with other
//! settings and holds it shared until it ends. This build retries until that build is done. Two
//! builds that do this to each other would wait forever, so a build gives up after a time limit when
//! another build also retries (see [`LockManager::lock_to_build`]).

use crate::compiler::job_queue::JobState;
use crate::util::flock;
use crate::{
    CargoResult,
    compiler::{BuildRunner, CompileKind, Unit},
};

use anyhow::{Context as _, bail};
use cargo_util::collections::HashMap;
use cargo_util::paths;
use std::{
    fmt::{Display, Formatter},
    fs::{File, OpenOptions, TryLockError},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock, RwLock},
    time::{Duration, Instant},
};
use tracing::instrument;

/// A struct to store the lock handles for build units during compilation.
pub struct LockManager {
    locks: RwLock<HashMap<LockKey, UnitLock>>,
    /// The lock file that builds hold shared while they retry a unit that another build uses
    /// with other settings. Created on first use.
    retrying: Mutex<Retrying>,
    retrying_path: OnceLock<PathBuf>,
}

struct UnitLock {
    file: File,
    /// Whether planning took the shared lock.
    locked: bool,
    /// Whether this unit or one of its dependencies was being compiled by another build during
    /// planning. Its fingerprint, or theirs, may have been read in the middle of that.
    contended: bool,
    /// Names the unit in "Blocking" messages.
    name: String,
}

#[derive(Default)]
struct Retrying {
    file: Option<File>,
    jobs: usize,
}

impl LockManager {
    pub fn new() -> Self {
        Self {
            locks: RwLock::new(HashMap::default()),
            retrying: Mutex::new(Retrying::default()),
            retrying_path: OnceLock::new(),
        }
    }

    /// Takes shared locks on `unit` and all its dependencies, without blocking, so that their
    /// fingerprints are not rewritten while planning reads them.
    ///
    /// Returns the unit's key, whether the unit is locked, and whether it is contended: another
    /// build was compiling the unit or one of its dependencies. A contended unit must be planned
    /// as dirty, and its job checks it again once it has waited for the other build.
    #[instrument(skip_all)]
    pub fn lock_shared(
        &self,
        build_runner: &BuildRunner<'_, '_>,
        unit: &Unit,
    ) -> CargoResult<(LockKey, bool, bool)> {
        let key = LockKey::from_unit(build_runner, unit);
        if let Some(lock) = self.locks.read().unwrap().get(&key) {
            return Ok((key, lock.locked, lock.contended));
        }
        self.retrying_path.get_or_init(|| {
            build_runner
                .files()
                .layout(CompileKind::Host)
                .build_dir()
                .root()
                .join(".cargo-unit-lock-retry")
        });

        let file = open(&key.0)?;
        let locked = try_lock(&key.0, &file, flock::try_lock_shared)?;
        let mut contended = !locked;
        for dep in build_runner.unit_deps(unit) {
            contended |= self.lock_shared(build_runner, &dep.unit)?.2;
        }
        let name = format!(
            "{} ({})",
            unit.pkg.name(),
            build_runner.files().unit_hash(unit)
        );
        self.locks.write().unwrap().insert(
            key.clone(),
            UnitLock {
                file,
                locked,
                contended,
                name,
            },
        );
        Ok((key, locked, contended))
    }

    /// Locks a unit that planning found dirty, from its job, before compiling it.
    ///
    /// Returns `true` with the unit locked exclusively, for the job to compile it, or `false` with
    /// it locked shared, when `recheck` is set and the unit's fingerprint now matches: another
    /// build compiled it as this one would have.
    ///
    /// It never blocks on the exclusive lock (see the module docs). While it waits, the job gives
    /// back its jobserver token and the build prints a "Blocking" message.
    #[instrument(skip(self, state))]
    pub fn lock_to_build(
        &self,
        state: &JobState<'_, '_>,
        key: &LockKey,
        recheck: bool,
    ) -> CargoResult<bool> {
        let locks = self.locks.read().unwrap();
        let Some(lock) = locks.get(key) else {
            bail!("lock was not found in lock manager: {key}");
        };
        let file = &lock.file;
        let fresh = || {
            if recheck {
                state.fresh_after_wait()
            } else {
                Ok(false)
            }
        };

        // Another build may have compiled the unit since planning, and be done with it.
        if try_lock(&key.0, file, flock::try_lock_exclusive)? {
            if fresh()? {
                flock::lock_shared(file)?;
                return Ok(false);
            }
            return Ok(true);
        }

        state.blocked_on_lock(&lock.name);
        let mut retry: Option<(RetryGuard<'_>, Option<Instant>)> = None;
        let mut pause = Duration::from_millis(10);
        loop {
            // This waits only for a build that compiles the unit right now.
            flock::lock_shared(file)?;
            if fresh()? {
                return Ok(false);
            }
            flock::unlock(file)?;
            if try_lock(&key.0, file, flock::try_lock_exclusive)? {
                state.resume_after_lock()?;
                return Ok(true);
            }

            // Another build holds the unit shared and uses it with other settings.
            let (_, both_since) = retry.get_or_insert_with(|| (self.start_retrying(), None));
            if self.others_retry()? {
                let since = both_since.get_or_insert_with(Instant::now);
                let limit = retry_limit();
                if since.elapsed() >= limit {
                    bail!(
                        "gave up waiting for {} after {}s: another cargo process uses it with \
                         different settings, and also waits for a unit that this build uses \
                         (lock {key})",
                        lock.name,
                        limit.as_secs(),
                    );
                }
            } else {
                *both_since = None;
            }
            std::thread::sleep(pause.mul_f64(0.5 + jitter()));
            pause = (pause * 2).min(Duration::from_millis(500));
        }
    }

    /// Downgrades an exclusive lock to a shared one.
    #[instrument(skip(self))]
    pub fn downgrade_to_shared(&self, key: &LockKey) -> CargoResult<()> {
        let locks = self.locks.read().unwrap();
        let Some(lock) = locks.get(key) else {
            bail!("lock was not found in lock manager: {key}");
        };
        flock::lock_shared(&lock.file)?;
        Ok(())
    }

    #[instrument(skip(self))]
    pub fn unlock(&self, key: &LockKey) -> CargoResult<()> {
        let locks = self.locks.read().unwrap();
        if let Some(lock) = locks.get(key) {
            flock::unlock(&lock.file)?;
        };

        Ok(())
    }

    /// Marks this build as one that retries a unit, for [`LockManager::others_retry`] in other
    /// builds, until the returned guard drops.
    fn start_retrying(&self) -> RetryGuard<'_> {
        let mut retrying = self.retrying.lock().unwrap();
        retrying.jobs += 1;
        if retrying.file.is_none()
            && let Some(path) = self.retrying_path.get()
            && let Ok(file) = open(path)
            && flock::lock_shared(&file).is_ok()
        {
            retrying.file = Some(file);
        }
        RetryGuard(self)
    }

    /// Whether another build also retries a unit. If so, the two may retry units that the other
    /// one holds, and neither would ever finish.
    fn others_retry(&self) -> CargoResult<bool> {
        let retrying = self.retrying.lock().unwrap();
        let (Some(file), Some(path)) = (&retrying.file, self.retrying_path.get()) else {
            return Ok(false);
        };
        // Our own shared lock would conflict with an exclusive try on another file handle, so
        // release it for the check.
        flock::unlock(file)?;
        let others = !try_lock(path, file, flock::try_lock_exclusive)?;
        flock::lock_shared(file)?;
        Ok(others)
    }
}

struct RetryGuard<'a>(&'a LockManager);

impl Drop for RetryGuard<'_> {
    fn drop(&mut self) {
        let mut retrying = self.0.retrying.lock().unwrap();
        retrying.jobs -= 1;
        if retrying.jobs == 0 {
            retrying.file = None;
        }
    }
}

fn open(path: &Path) -> CargoResult<File> {
    if let Some(parent) = path.parent() {
        paths::create_dir_all(parent)?;
    }
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(path)
        .with_context(|| format!("failed to open: {}", path.display()))
}

/// Tries a lock without blocking; `true` when it is taken, or when the filesystem does not
/// support locking (see [`flock::try_acquire`]).
fn try_lock(
    path: &Path,
    file: &File,
    lock: fn(&File) -> Result<(), TryLockError>,
) -> CargoResult<bool> {
    flock::try_acquire(path, &|| lock(file))
}

/// A number in [0, 1) that differs between processes and calls, so that builds that retry the
/// same unit do not keep trying at the same moments.
fn jitter() -> f64 {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u32(std::process::id());
    (hasher.finish() >> 11) as f64 / (1u64 << 53) as f64
}

/// How long a build retries a unit while another build retries one too: 60 s, or
/// `__CARGO_TEST_FINE_GRAIN_LOCK_TIMEOUT_MS` for the testsuite.
#[expect(
    clippy::disallowed_methods,
    reason = "testing only, no reason for config support"
)]
fn retry_limit() -> Duration {
    std::env::var("__CARGO_TEST_FINE_GRAIN_LOCK_TIMEOUT_MS")
        .ok()
        .and_then(|ms| ms.parse().ok())
        .map(Duration::from_millis)
        .unwrap_or(Duration::from_secs(60))
}

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct LockKey(PathBuf);

impl LockKey {
    fn from_unit(build_runner: &BuildRunner<'_, '_>, unit: &Unit) -> Self {
        Self(build_runner.files().build_unit_lock(unit))
    }
}

impl Display for LockKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.display())
    }
}
