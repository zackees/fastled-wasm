use std::fs::{self, OpenOptions};
use std::io::{self, IsTerminal};
use std::path::Path;

use anyhow::{Context, Result};
use fs2::FileExt;

use crate::path::NormalizedPath;

const ROOT_ENV_VAR: &str = "FASTLED_HOME";
const CACHE_DIR: &str = "cache";
const STATE_DIR: &str = "state";
const LOCK_FILE: &str = "cache-upgrade.lock";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CacheUpgradeOutcome {
    pub(crate) cache_removed: bool,
    pub(crate) warning: Option<String>,
}

fn fastled_root() -> Result<NormalizedPath> {
    if let Some(root) = std::env::var_os(ROOT_ENV_VAR) {
        return Ok(NormalizedPath::new(root));
    }
    Ok(NormalizedPath::new(
        dirs::home_dir()
            .context("cannot resolve home directory for cache upgrade cleanup")?
            .join(".fastled"),
    ))
}

fn breadcrumb_path(root: &Path, version: &str) -> NormalizedPath {
    NormalizedPath::new(
        root.join(STATE_DIR)
            .join(format!("cache-cleared-v{version}")),
    )
}

fn breadcrumb_temp_path(root: &Path, version: &str) -> NormalizedPath {
    NormalizedPath::new(
        root.join(STATE_DIR)
            .join(format!(".cache-cleared-v{version}.tmp")),
    )
}

fn breadcrumb_matches(path: &Path, version: &str) -> bool {
    fs::read_to_string(path).is_ok_and(|contents| contents.trim() == version)
}

fn publish_breadcrumb(root: &Path, version: &str) -> Result<()> {
    let temporary = breadcrumb_temp_path(root, version);
    let published = breadcrumb_path(root, version);
    fs::write(&temporary, format!("{version}\n"))
        .with_context(|| format!("write cache upgrade breadcrumb {}", temporary.display()))?;
    if let Err(error) = fs::rename(&temporary, &published) {
        let _ = fs::remove_file(&temporary);
        return Err(error)
            .with_context(|| format!("publish cache upgrade breadcrumb {}", published.display()));
    }
    Ok(())
}

fn ensure_cache_version_inner<Remove, Warn>(
    root: &Path,
    version: &str,
    remove_cache: Remove,
    mut warn: Warn,
) -> Result<CacheUpgradeOutcome>
where
    Remove: FnOnce(&Path) -> io::Result<()>,
    Warn: FnMut(&str),
{
    let marker = breadcrumb_path(root, version);
    if breadcrumb_matches(&marker, version) {
        return Ok(CacheUpgradeOutcome {
            cache_removed: false,
            warning: None,
        });
    }

    let state_dir = root.join(STATE_DIR);
    fs::create_dir_all(&state_dir).with_context(|| {
        format!(
            "create cache upgrade state directory {}",
            state_dir.display()
        )
    })?;
    let lock_path = state_dir.join(LOCK_FILE);
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .with_context(|| format!("open cache upgrade lock {}", lock_path.display()))?;
    FileExt::lock_exclusive(&lock)
        .with_context(|| format!("lock cache upgrade state {}", lock_path.display()))?;

    // Another process may have completed the cleanup while this process was
    // waiting for the lock. Re-read the breadcrumb inside the critical section.
    if breadcrumb_matches(&marker, version) {
        return Ok(CacheUpgradeOutcome {
            cache_removed: false,
            warning: None,
        });
    }

    let cache = root.join(CACHE_DIR);
    let warning = if cache.exists() {
        let warning = format!(
            "warning: FastLED CLI was updated to {version}; removing cache at {}",
            cache.display()
        );
        warn(&warning);
        remove_cache(&cache)
            .with_context(|| format!("remove outdated FastLED cache {}", cache.display()))?;
        Some(warning)
    } else {
        None
    };

    publish_breadcrumb(root, version)?;
    Ok(CacheUpgradeOutcome {
        cache_removed: warning.is_some(),
        warning,
    })
}

#[cfg(test)]
pub(crate) fn ensure_cache_version(root: &Path, version: &str) -> Result<CacheUpgradeOutcome> {
    ensure_cache_version_inner(root, version, |cache| fs::remove_dir_all(cache), |_| {})
}

#[cfg(test)]
fn ensure_cache_version_with<Remove>(
    root: &Path,
    version: &str,
    remove_cache: Remove,
) -> Result<CacheUpgradeOutcome>
where
    Remove: FnOnce(&Path) -> io::Result<()>,
{
    ensure_cache_version_inner(root, version, remove_cache, |_| {})
}

pub(crate) fn render_warning(warning: &str, terminal: bool) -> String {
    if terminal {
        format!("\u{1b}[38;5;11m{warning}\u{1b}[0m")
    } else {
        warning.to_string()
    }
}

pub(crate) fn enforce_current_cache_version() -> Result<()> {
    let root = fastled_root()?;
    ensure_cache_version_inner(
        &root,
        env!("CARGO_PKG_VERSION"),
        |cache| fs::remove_dir_all(cache),
        |warning| {
            eprintln!(
                "{}",
                render_warning(warning, std::io::stderr().is_terminal())
            );
        },
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io;
    use std::sync::{Arc, Barrier};
    use std::thread;

    fn write(path: &std::path::Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn first_version_clears_only_shared_cache_and_records_breadcrumb() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        write(&root.join("cache/legacy.txt"), "stale");
        write(&root.join("toolchains/keep.txt"), "toolchain");
        write(&root.join("run/keep.txt"), "runtime");
        write(&root.join("state/keep.txt"), "state");

        let outcome = ensure_cache_version(root, "2.0.16").unwrap();

        let expected_warning = format!(
            "warning: FastLED CLI was updated to 2.0.16; removing cache at {}",
            root.join("cache").display()
        );
        assert!(outcome.cache_removed);
        assert_eq!(outcome.warning.as_deref(), Some(expected_warning.as_str()));
        assert!(!root.join("cache").exists());
        assert!(root.join("toolchains/keep.txt").is_file());
        assert!(root.join("run/keep.txt").is_file());
        assert!(root.join("state/keep.txt").is_file());
        assert_eq!(
            fs::read_to_string(breadcrumb_path(root, "2.0.16")).unwrap(),
            "2.0.16\n"
        );
    }

    #[test]
    fn same_version_preserves_new_cache_and_emits_no_warning() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        write(&root.join("cache/legacy.txt"), "stale");
        ensure_cache_version(root, "2.0.16").unwrap();
        write(&root.join("cache/fresh.txt"), "fresh");

        let outcome = ensure_cache_version(root, "2.0.16").unwrap();

        assert!(!outcome.cache_removed);
        assert_eq!(outcome.warning, None);
        assert_eq!(
            fs::read_to_string(root.join("cache/fresh.txt")).unwrap(),
            "fresh"
        );
    }

    #[test]
    fn changed_version_clears_cache_once_again() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        ensure_cache_version(root, "2.0.16").unwrap();
        write(&root.join("cache/from-old-version.txt"), "stale");

        let outcome = ensure_cache_version(root, "2.0.17").unwrap();

        assert!(outcome.cache_removed);
        assert!(outcome.warning.unwrap().contains("updated to 2.0.17"));
        assert!(!root.join("cache").exists());
        assert_eq!(
            fs::read_to_string(breadcrumb_path(root, "2.0.17")).unwrap(),
            "2.0.17\n"
        );
    }

    #[test]
    fn missing_cache_records_version_silently() {
        let temp = tempfile::tempdir().unwrap();

        let outcome = ensure_cache_version(temp.path(), "2.0.16").unwrap();

        assert!(!outcome.cache_removed);
        assert_eq!(outcome.warning, None);
        assert_eq!(
            fs::read_to_string(breadcrumb_path(temp.path(), "2.0.16")).unwrap(),
            "2.0.16\n"
        );
    }

    #[test]
    fn deletion_failure_does_not_publish_breadcrumb() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        write(&root.join("cache/legacy.txt"), "stale");

        let result = ensure_cache_version_with(root, "2.0.16", |_cache| {
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "locked"))
        });

        assert!(result.is_err());
        assert!(root.join("cache/legacy.txt").is_file());
        assert!(!breadcrumb_path(root, "2.0.16").exists());
    }

    #[test]
    fn breadcrumb_failure_does_not_publish_success() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        write(&root.join("cache/legacy.txt"), "stale");
        fs::create_dir_all(breadcrumb_temp_path(root, "2.0.16")).unwrap();

        let result = ensure_cache_version(root, "2.0.16");

        assert!(result.is_err());
        assert!(!breadcrumb_path(root, "2.0.16").exists());
    }

    #[test]
    fn concurrent_first_runs_clear_once_after_rechecking_under_lock() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        write(&root.join("cache/legacy.txt"), "stale");
        let barrier = Arc::new(Barrier::new(3));

        let handles = (0..2)
            .map(|_| {
                let root = root.clone();
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    ensure_cache_version(&root, "2.0.16").unwrap()
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();
        let outcomes = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();

        assert_eq!(outcomes.iter().filter(|item| item.cache_removed).count(), 1);
        assert_eq!(
            outcomes
                .iter()
                .filter(|item| item.warning.is_some())
                .count(),
            1
        );
        assert_eq!(
            fs::read_to_string(breadcrumb_path(&root, "2.0.16")).unwrap(),
            "2.0.16\n"
        );
    }

    #[test]
    fn warning_rendering_is_yellow_only_for_terminals() {
        let warning = "warning: FastLED CLI was updated to 2.0.16; removing cache";

        assert_eq!(render_warning(warning, false), warning);
        let colored = render_warning(warning, true);
        assert!(colored.starts_with("\u{1b}[38;5;11m"), "{colored:?}");
        assert!(colored.contains(warning));
        assert!(colored.ends_with("\u{1b}[0m"), "{colored:?}");
    }
}
