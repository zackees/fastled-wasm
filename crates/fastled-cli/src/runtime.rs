//! Managed runtime handoff for self-updating executable installs.
//!
//! The Python package and direct release zips may launch an older executable
//! from their install location. At process start, this module moves execution
//! into `~/.fastled/run/fastled-v<version>-<hash>/...`, prefers newer binaries
//! dropped into `~/.fastled/update/`, and periodically removes stale copies.

use crate::archive;
use anyhow::{Context, Result};
use fs2::FileExt;
use std::cmp::Ordering;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

const ROOT_ENV_VAR: &str = "FASTLED_HOME";
const MANAGED_RUNTIME_ENV_VAR: &str = "FASTLED_MANAGED_RUNTIME";
const ORIGINAL_EXE_ENV_VAR: &str = "FASTLED_ORIGINAL_EXE";
const RUN_DIR: &str = "run";
const UPDATE_DIR: &str = "update";
const LOCK_FILENAME: &str = ".lock";
const GC_MARKER_FILENAME: &str = ".last-gc";
const LAST_USED_FILENAME: &str = "last-used";
const GC_INTERVAL_SECONDS: u64 = 24 * 60 * 60;
const STALE_RUN_SECONDS: u64 = 14 * 24 * 60 * 60;
const STALE_UPDATE_SECONDS: u64 = 7 * 24 * 60 * 60;

#[cfg(windows)]
const FASTLED_EXE_NAMES: &[&str] = &["fastled.exe"];
#[cfg(not(windows))]
const FASTLED_EXE_NAMES: &[&str] = &["fastled"];

#[derive(Clone, Debug, PartialEq, Eq)]
struct ParsedVersion(Vec<u64>);

impl ParsedVersion {
    fn parse(raw: &str) -> Option<Self> {
        let parts = raw
            .split('.')
            .map(|part| part.parse::<u64>().ok())
            .collect::<Option<Vec<_>>>()?;
        if parts.is_empty() {
            return None;
        }
        Some(Self(parts))
    }
}

impl Ord for ParsedVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        let len = self.0.len().max(other.0.len());
        for index in 0..len {
            let left = self.0.get(index).copied().unwrap_or(0);
            let right = other.0.get(index).copied().unwrap_or(0);
            match left.cmp(&right) {
                Ordering::Equal => {}
                ordering => return ordering,
            }
        }
        Ordering::Equal
    }
}

impl PartialOrd for ParsedVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Debug)]
struct RuntimeCandidate {
    path: PathBuf,
    version: ParsedVersion,
    version_text: String,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct GcSummary {
    scanned_run_dirs: usize,
    removed_run_dirs: usize,
    skipped_current_run_dirs: usize,
    skipped_fresh_run_dirs: usize,
    failed_run_dirs: usize,
    scanned_update_files: usize,
    removed_update_files: usize,
    failed_update_files: usize,
}

/// Re-execute from the managed global runtime if needed.
///
/// Returns `Ok(Some(code))` after the replacement process exits on Windows.
/// On Unix the replacement process is executed in-place, so successful
/// re-exec does not return.
pub fn maybe_reexec_from_managed_runtime(raw_args: &[OsString]) -> Result<Option<i32>> {
    if std::env::var_os(MANAGED_RUNTIME_ENV_VAR).is_some() {
        return Ok(None);
    }

    let current_exe = std::env::current_exe().context("failed to determine current executable")?;
    let paths = RuntimePaths::new()?;

    // Free the install-location image (venv, pip bin dir) so it can be
    // deleted while we run. Managed-runtime copies under run/update are
    // already disposable and handled by GC, so skip those.
    if !path_is_under(&current_exe, &paths.run_root)
        && !path_is_under(&current_exe, &paths.update_root)
    {
        crate::install_unlock::unlock_install_exe(&current_exe);
    }

    fs::create_dir_all(&paths.run_root)
        .with_context(|| format!("cannot create {}", paths.run_root.display()))?;
    fs::create_dir_all(&paths.update_root)
        .with_context(|| format!("cannot create {}", paths.update_root.display()))?;

    let current_version = ParsedVersion::parse(env!("CARGO_PKG_VERSION"))
        .context("crate version is not a dotted numeric version")?;

    if let Some(update) = find_best_pending_update(&paths.update_root, &current_version)? {
        let installed = install_update_candidate(&paths, &current_exe, &update)?;
        run_periodic_gc(&paths, installed.parent());
        return launch_replacement(&installed, raw_args, &current_exe);
    }

    if let Some(newer_runtime) = find_best_runtime_candidate(&paths.run_root, &current_version)? {
        touch_last_used_for_exe(&newer_runtime.path)?;
        run_periodic_gc(&paths, newer_runtime.path.parent());
        return launch_replacement(&newer_runtime.path, raw_args, &current_exe);
    }

    if path_is_under(&current_exe, &paths.run_root) {
        touch_last_used_for_exe(&current_exe)?;
        run_periodic_gc(&paths, current_exe.parent());
        return Ok(None);
    }

    let relocated = ensure_runtime_copy(&paths, &current_exe, env!("CARGO_PKG_VERSION"))?;
    run_periodic_gc(&paths, relocated.parent());
    launch_replacement(&relocated, raw_args, &current_exe)
}

#[derive(Debug)]
struct RuntimePaths {
    run_root: PathBuf,
    update_root: PathBuf,
}

impl RuntimePaths {
    fn new() -> Result<Self> {
        let root = if let Some(root) = std::env::var_os(ROOT_ENV_VAR) {
            PathBuf::from(root)
        } else {
            dirs::home_dir()
                .context("could not determine home directory")?
                .join(".fastled")
        };
        Ok(Self {
            run_root: root.join(RUN_DIR),
            update_root: root.join(UPDATE_DIR),
        })
    }

    #[cfg(test)]
    fn with_root(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            run_root: root.join(RUN_DIR),
            update_root: root.join(UPDATE_DIR),
        }
    }
}

fn find_best_pending_update(
    update_root: &Path,
    current_version: &ParsedVersion,
) -> Result<Option<RuntimeCandidate>> {
    let mut candidates = Vec::new();
    if !update_root.is_dir() {
        return Ok(None);
    }

    for entry in fs::read_dir(update_root)
        .with_context(|| format!("cannot read {}", update_root.display()))?
        .flatten()
    {
        let path = entry.path();
        if path.is_file() {
            maybe_push_update_file(&mut candidates, &path, current_version);
            continue;
        }

        if !path.is_dir() {
            continue;
        }

        let Some((version_text, version)) = version_from_path_name(&path) else {
            continue;
        };
        if version <= *current_version {
            continue;
        }
        for nested in fs::read_dir(&path)
            .with_context(|| format!("cannot read {}", path.display()))?
            .flatten()
        {
            let nested_path = nested.path();
            if nested_path.is_file() && is_runnable_update_file(&nested_path) {
                candidates.push(RuntimeCandidate {
                    path: nested_path,
                    version: version.clone(),
                    version_text: version_text.clone(),
                });
            }
        }
    }

    Ok(best_candidate(candidates))
}

fn maybe_push_update_file(
    candidates: &mut Vec<RuntimeCandidate>,
    path: &Path,
    current_version: &ParsedVersion,
) {
    if !is_runnable_update_file(path) {
        return;
    }
    let Some((version_text, version)) = version_from_path_name(path) else {
        return;
    };
    if version <= *current_version {
        return;
    }
    candidates.push(RuntimeCandidate {
        path: path.to_path_buf(),
        version,
        version_text,
    });
}

fn find_best_runtime_candidate(
    run_root: &Path,
    current_version: &ParsedVersion,
) -> Result<Option<RuntimeCandidate>> {
    let mut candidates = Vec::new();
    if !run_root.is_dir() {
        return Ok(None);
    }

    for entry in fs::read_dir(run_root)
        .with_context(|| format!("cannot read {}", run_root.display()))?
        .flatten()
    {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some((version_text, version)) = version_from_path_name(&path) else {
            continue;
        };
        if version <= *current_version {
            continue;
        }
        for exe_name in FASTLED_EXE_NAMES {
            let candidate = path.join(exe_name);
            if candidate.is_file() {
                candidates.push(RuntimeCandidate {
                    path: candidate,
                    version: version.clone(),
                    version_text: version_text.clone(),
                });
            }
        }
    }

    Ok(best_candidate(candidates))
}

fn best_candidate(candidates: Vec<RuntimeCandidate>) -> Option<RuntimeCandidate> {
    candidates
        .into_iter()
        .max_by(|left, right| match left.version.cmp(&right.version) {
            Ordering::Equal => left.path.cmp(&right.path),
            ordering => ordering,
        })
}

fn install_update_candidate(
    paths: &RuntimePaths,
    current_exe: &Path,
    update: &RuntimeCandidate,
) -> Result<PathBuf> {
    let _lock = lock_runtime_root(&paths.run_root)?;
    let hash = archive::sha256_file(&update.path)
        .with_context(|| format!("cannot hash update {}", update.path.display()))?;
    let dest_dir = paths
        .run_root
        .join(format!("fastled-v{}-{hash}", update.version_text));
    fs::create_dir_all(&dest_dir)
        .with_context(|| format!("cannot create {}", dest_dir.display()))?;
    let dest = dest_dir.join(runtime_exe_name(current_exe));

    copy_executable_atomically(&update.path, &dest, &hash)?;
    let _ = fs::remove_file(&update.path);
    touch_last_used(&dest_dir)?;
    Ok(dest)
}

fn ensure_runtime_copy(paths: &RuntimePaths, current_exe: &Path, version: &str) -> Result<PathBuf> {
    let _lock = lock_runtime_root(&paths.run_root)?;
    let hash = archive::sha256_file(current_exe)
        .with_context(|| format!("cannot hash {}", current_exe.display()))?;
    let dest_dir = paths.run_root.join(format!("fastled-v{version}-{hash}"));
    fs::create_dir_all(&dest_dir)
        .with_context(|| format!("cannot create {}", dest_dir.display()))?;
    let dest = dest_dir.join(runtime_exe_name(current_exe));

    if exe_hash_matches(&dest, &hash) {
        touch_last_used(&dest_dir)?;
        return Ok(dest);
    }

    copy_executable_atomically(current_exe, &dest, &hash)?;
    touch_last_used(&dest_dir)?;
    Ok(dest)
}

fn runtime_exe_name(current_exe: &Path) -> &OsStr {
    current_exe
        .file_name()
        .filter(|name| {
            let name = name.to_string_lossy();
            FASTLED_EXE_NAMES
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(&name))
        })
        .unwrap_or_else(|| OsStr::new(FASTLED_EXE_NAMES[0]))
}

fn copy_executable_atomically(source: &Path, dest: &Path, expected_hash: &str) -> Result<()> {
    if exe_hash_matches(dest, expected_hash) {
        return Ok(());
    }

    let file_name = dest
        .file_name()
        .context("destination executable path has no filename")?;
    let temp = dest.with_file_name(format!(
        ".{}.{}.tmp",
        file_name.to_string_lossy(),
        std::process::id()
    ));
    let _ = fs::remove_file(&temp);
    fs::copy(source, &temp)
        .with_context(|| format!("cannot copy {} to {}", source.display(), temp.display()))?;
    let permissions = fs::metadata(source)
        .with_context(|| format!("cannot stat {}", source.display()))?
        .permissions();
    fs::set_permissions(&temp, permissions)
        .with_context(|| format!("cannot set permissions on {}", temp.display()))?;

    if dest.exists() && !exe_hash_matches(dest, expected_hash) {
        let _ = fs::remove_file(dest);
    }

    match fs::rename(&temp, dest) {
        Ok(()) => Ok(()),
        Err(_err) if exe_hash_matches(dest, expected_hash) => {
            let _ = fs::remove_file(&temp);
            Ok(())
        }
        Err(err) => {
            let _ = fs::remove_file(&temp);
            Err(err).with_context(|| format!("cannot replace {}", dest.display()))
        }
    }
}

fn launch_replacement(
    executable: &Path,
    raw_args: &[OsString],
    original_exe: &Path,
) -> Result<Option<i32>> {
    let mut command = Command::new(executable);
    command
        .args(raw_args.iter().skip(1))
        .env(MANAGED_RUNTIME_ENV_VAR, "1")
        .env(ORIGINAL_EXE_ENV_VAR, original_exe);

    #[cfg(unix)]
    {
        let err = command.exec();
        Err(err).with_context(|| format!("failed to exec {}", executable.display()))
    }

    #[cfg(not(unix))]
    {
        let status = command
            .status()
            .with_context(|| format!("failed to launch {}", executable.display()))?;
        Ok(Some(status.code().unwrap_or(1)))
    }
}

fn run_periodic_gc(paths: &RuntimePaths, current_dir: Option<&Path>) {
    let Ok(now) = current_unix_seconds() else {
        return;
    };
    let _ = maybe_run_periodic_gc_at(
        paths,
        current_dir,
        now,
        GC_INTERVAL_SECONDS,
        STALE_RUN_SECONDS,
        STALE_UPDATE_SECONDS,
    );
}

fn maybe_run_periodic_gc_at(
    paths: &RuntimePaths,
    current_dir: Option<&Path>,
    now: u64,
    interval_seconds: u64,
    stale_run_seconds: u64,
    stale_update_seconds: u64,
) -> Result<Option<GcSummary>> {
    fs::create_dir_all(&paths.run_root)
        .with_context(|| format!("cannot create {}", paths.run_root.display()))?;
    if !gc_due(&paths.run_root, now, interval_seconds) {
        return Ok(None);
    }

    let _lock = lock_runtime_root(&paths.run_root)?;
    if !gc_due(&paths.run_root, now, interval_seconds) {
        return Ok(None);
    }

    let mut summary = purge_stale_run_dirs(&paths.run_root, current_dir, now, stale_run_seconds)?;
    let update_summary = purge_stale_update_files(&paths.update_root, now, stale_update_seconds)?;
    summary.scanned_update_files = update_summary.scanned_update_files;
    summary.removed_update_files = update_summary.removed_update_files;
    summary.failed_update_files = update_summary.failed_update_files;

    fs::write(paths.run_root.join(GC_MARKER_FILENAME), now.to_string())
        .with_context(|| format!("cannot write {}", paths.run_root.display()))?;
    Ok(Some(summary))
}

fn gc_due(run_root: &Path, now: u64, interval_seconds: u64) -> bool {
    let marker = run_root.join(GC_MARKER_FILENAME);
    let Ok(raw) = fs::read_to_string(marker) else {
        return true;
    };
    let Ok(last_run) = raw.trim().parse::<u64>() else {
        return true;
    };
    now.saturating_sub(last_run) >= interval_seconds
}

fn purge_stale_run_dirs(
    run_root: &Path,
    current_dir: Option<&Path>,
    now: u64,
    stale_seconds: u64,
) -> Result<GcSummary> {
    let mut summary = GcSummary::default();
    let cutoff = now.saturating_sub(stale_seconds);

    for entry in fs::read_dir(run_root)
        .with_context(|| format!("cannot read {}", run_root.display()))?
        .flatten()
    {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        summary.scanned_run_dirs += 1;
        if current_dir.is_some_and(|current| same_path(current, &path)) {
            summary.skipped_current_run_dirs += 1;
            continue;
        }

        let last_used = runtime_copy_last_used(&path).unwrap_or(now);
        if last_used > cutoff {
            summary.skipped_fresh_run_dirs += 1;
            continue;
        }

        match fs::remove_dir_all(&path) {
            Ok(()) => summary.removed_run_dirs += 1,
            Err(_) => summary.failed_run_dirs += 1,
        }
    }

    Ok(summary)
}

fn purge_stale_update_files(update_root: &Path, now: u64, stale_seconds: u64) -> Result<GcSummary> {
    let mut summary = GcSummary::default();
    if !update_root.is_dir() {
        return Ok(summary);
    }

    let cutoff = now.saturating_sub(stale_seconds);
    purge_stale_update_files_in_dir(update_root, cutoff, &mut summary)?;
    Ok(summary)
}

fn purge_stale_update_files_in_dir(dir: &Path, cutoff: u64, summary: &mut GcSummary) -> Result<()> {
    for entry in fs::read_dir(dir)
        .with_context(|| format!("cannot read {}", dir.display()))?
        .flatten()
    {
        let path = entry.path();
        if path.is_dir() {
            purge_stale_update_files_in_dir(&path, cutoff, summary)?;
            let _ = fs::remove_dir(&path);
            continue;
        }
        if !path.is_file() {
            continue;
        }
        summary.scanned_update_files += 1;
        let modified = fs::metadata(&path)
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(system_time_to_unix_seconds)
            .unwrap_or(u64::MAX);
        if modified > cutoff {
            continue;
        }
        match fs::remove_file(&path) {
            Ok(()) => summary.removed_update_files += 1,
            Err(_) => summary.failed_update_files += 1,
        }
    }
    Ok(())
}

fn lock_runtime_root(run_root: &Path) -> Result<File> {
    fs::create_dir_all(run_root)
        .with_context(|| format!("cannot create {}", run_root.display()))?;
    let lock_path = run_root.join(LOCK_FILENAME);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("cannot open {}", lock_path.display()))?;
    file.lock_exclusive()
        .with_context(|| format!("cannot lock {}", lock_path.display()))?;
    Ok(file)
}

fn exe_hash_matches(path: &Path, expected_hash: &str) -> bool {
    path.is_file()
        && archive::sha256_file(path).is_ok_and(|actual| actual.eq_ignore_ascii_case(expected_hash))
}

fn touch_last_used_for_exe(exe: &Path) -> Result<()> {
    let dir = exe
        .parent()
        .with_context(|| format!("{} has no parent directory", exe.display()))?;
    touch_last_used(dir)
}

fn touch_last_used(dir: &Path) -> Result<()> {
    fs::write(
        dir.join(LAST_USED_FILENAME),
        current_unix_seconds()?.to_string(),
    )
    .with_context(|| format!("cannot write last-used in {}", dir.display()))?;
    Ok(())
}

fn runtime_copy_last_used(path: &Path) -> Option<u64> {
    fs::read_to_string(path.join(LAST_USED_FILENAME))
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .or_else(|| {
            fs::metadata(path)
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(system_time_to_unix_seconds)
        })
}

fn current_unix_seconds() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .context("system clock is before unix epoch")
}

fn system_time_to_unix_seconds(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn path_is_under(path: &Path, root: &Path) -> bool {
    match (fs::canonicalize(path), fs::canonicalize(root)) {
        (Ok(path), Ok(root)) => path.starts_with(root),
        _ => path.starts_with(root),
    }
}

fn is_runnable_update_file(path: &Path) -> bool {
    #[cfg(windows)]
    {
        path.extension()
            .and_then(OsStr::to_str)
            .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    }
    #[cfg(not(windows))]
    {
        path.file_name()
            .and_then(OsStr::to_str)
            .is_some_and(|name| {
                FASTLED_EXE_NAMES
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(name))
                    || version_from_name(name).is_some()
            })
    }
}

fn version_from_path_name(path: &Path) -> Option<(String, ParsedVersion)> {
    let name = path.file_name()?.to_string_lossy();
    version_from_name(&name)
}

fn version_from_name(name: &str) -> Option<(String, ParsedVersion)> {
    const PREFIX: &str = "fastled-v";
    let prefix = name.get(..PREFIX.len())?;
    if !prefix.eq_ignore_ascii_case(PREFIX) {
        return None;
    }

    let mut version_text = String::new();
    for ch in name[PREFIX.len()..].chars() {
        if ch.is_ascii_digit() || ch == '.' {
            version_text.push(ch);
        } else {
            break;
        }
    }
    while version_text.ends_with('.') {
        version_text.pop();
    }
    if version_text.is_empty() {
        return None;
    }
    let parsed = ParsedVersion::parse(&version_text)?;
    Some((version_text, parsed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn write_file(path: &Path, bytes: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        let mut file = File::create(path).expect("create file");
        file.write_all(bytes).expect("write file");
    }

    fn candidate_name(version: &str) -> String {
        #[cfg(windows)]
        {
            format!("fastled-v{version}.exe")
        }
        #[cfg(not(windows))]
        {
            format!("fastled-v{version}")
        }
    }

    #[test]
    fn version_parser_orders_semver_segments_numerically() {
        let v209 = ParsedVersion::parse("2.0.9").unwrap();
        let v2010 = ParsedVersion::parse("2.0.10").unwrap();
        let v210 = ParsedVersion::parse("2.1").unwrap();

        assert!(v2010 > v209);
        assert!(v210 > v2010);
        assert_eq!(
            ParsedVersion::parse("2.1.0").unwrap().cmp(&v210),
            Ordering::Equal
        );
    }

    #[test]
    fn extracts_version_from_update_filename() {
        let (text, version) = version_from_name("fastled-v2.0.10.exe").unwrap();
        assert_eq!(text, "2.0.10");
        assert_eq!(version, ParsedVersion::parse("2.0.10").unwrap());
    }

    #[test]
    fn pending_update_prefers_newest_version() {
        let temp = TempDir::new().expect("tempdir");
        let update_root = temp.path().join(UPDATE_DIR);
        write_file(&update_root.join(candidate_name("2.0.8")), b"old");
        write_file(&update_root.join(candidate_name("2.1.0")), b"new");

        let current = ParsedVersion::parse("2.0.7").unwrap();
        let found = find_best_pending_update(&update_root, &current)
            .expect("find update")
            .expect("candidate");

        assert!(found.path.ends_with(candidate_name("2.1.0")));
        assert_eq!(found.version_text, "2.1.0");
    }

    #[test]
    fn pending_update_can_live_inside_versioned_directory() {
        let temp = TempDir::new().expect("tempdir");
        let update_root = temp.path().join(UPDATE_DIR);
        let nested = update_root.join("fastled-v9.0.0");
        write_file(&nested.join(FASTLED_EXE_NAMES[0]), b"new");

        let current = ParsedVersion::parse("2.0.7").unwrap();
        let found = find_best_pending_update(&update_root, &current)
            .expect("find update")
            .expect("candidate");

        assert_eq!(found.version_text, "9.0.0");
        assert!(found.path.ends_with(FASTLED_EXE_NAMES[0]));
    }

    #[test]
    fn runtime_candidate_prefers_newer_run_copy() {
        let temp = TempDir::new().expect("tempdir");
        let run_root = temp.path().join(RUN_DIR);
        write_file(
            &run_root
                .join("fastled-v2.0.8-hash")
                .join(FASTLED_EXE_NAMES[0]),
            b"old",
        );
        write_file(
            &run_root
                .join("fastled-v2.2.0-hash")
                .join(FASTLED_EXE_NAMES[0]),
            b"new",
        );

        let current = ParsedVersion::parse("2.0.7").unwrap();
        let found = find_best_runtime_candidate(&run_root, &current)
            .expect("find runtime")
            .expect("candidate");

        assert_eq!(found.version_text, "2.2.0");
    }

    #[test]
    fn ensure_runtime_copy_copies_to_hash_keyed_run_dir() {
        let temp = TempDir::new().expect("tempdir");
        let paths = RuntimePaths::with_root(temp.path().join("home"));
        let source = temp.path().join(FASTLED_EXE_NAMES[0]);
        write_file(&source, b"binary");

        let relocated = ensure_runtime_copy(&paths, &source, "2.0.7").expect("runtime copy");
        let hash = archive::sha256_file(&source).expect("hash source");

        assert!(relocated.is_file());
        assert_eq!(fs::read(&relocated).expect("read relocated"), b"binary");
        assert!(relocated
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains(&hash));
        assert!(relocated
            .parent()
            .unwrap()
            .join(LAST_USED_FILENAME)
            .is_file());
    }

    #[test]
    fn install_update_moves_candidate_into_run_dir() {
        let temp = TempDir::new().expect("tempdir");
        let paths = RuntimePaths::with_root(temp.path().join("home"));
        let current = temp.path().join(FASTLED_EXE_NAMES[0]);
        let update = paths.update_root.join(candidate_name("3.0.0"));
        write_file(&current, b"current");
        write_file(&update, b"updated");

        let candidate = RuntimeCandidate {
            path: update.clone(),
            version: ParsedVersion::parse("3.0.0").unwrap(),
            version_text: "3.0.0".to_string(),
        };
        let installed = install_update_candidate(&paths, &current, &candidate).expect("install");

        assert!(installed.is_file());
        assert_eq!(fs::read(&installed).expect("read installed"), b"updated");
        assert!(!update.exists());
        assert!(installed
            .parent()
            .unwrap()
            .join(LAST_USED_FILENAME)
            .is_file());
    }

    #[test]
    fn periodic_gc_removes_stale_run_dirs_and_skips_current() {
        let temp = TempDir::new().expect("tempdir");
        let paths = RuntimePaths::with_root(temp.path().join("home"));
        fs::create_dir_all(&paths.run_root).expect("create run root");
        fs::create_dir_all(&paths.update_root).expect("create update root");
        let stale = paths.run_root.join("fastled-v1.0.0-stale");
        let current = paths.run_root.join("fastled-v2.0.7-current");
        fs::create_dir_all(&stale).expect("stale dir");
        fs::create_dir_all(&current).expect("current dir");
        fs::write(stale.join(LAST_USED_FILENAME), "10").expect("stale last used");
        fs::write(current.join(LAST_USED_FILENAME), "10").expect("current last used");

        let summary = maybe_run_periodic_gc_at(&paths, Some(&current), 100, 0, 50, 50)
            .expect("gc")
            .expect("summary");

        assert_eq!(summary.removed_run_dirs, 1);
        assert_eq!(summary.skipped_current_run_dirs, 1);
        assert!(!stale.exists());
        assert!(current.exists());
    }
}
