use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::path::NormalizedPath;

const CACHE_SCHEMA: u32 = 1;
const METADATA_FILE: &str = "cache-metadata.json";

pub(crate) const DYNAMIC_LOADER_JS: &str = include_str!("../assets/dynamic_loader.js");

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct FingerprintSpec {
    root: NormalizedPath,
    include: Vec<String>,
    exclude: Vec<String>,
}

struct WatchedFingerprint {
    value: String,
    observed_generation: u64,
    generation: Arc<AtomicU64>,
    _watcher: RecommendedWatcher,
}

static FINGERPRINT_CACHE: OnceLock<
    Mutex<std::collections::HashMap<FingerprintSpec, WatchedFingerprint>>,
> = OnceLock::new();

fn build_glob_set(patterns: &[String], default_all: bool) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    if patterns.is_empty() && default_all {
        builder.add(Glob::new("**/*")?);
    } else {
        for pattern in patterns {
            builder.add(Glob::new(pattern)?);
        }
    }
    Ok(builder.build()?)
}

fn mark_fingerprint_dirty(generation: &AtomicU64) {
    generation.fetch_add(1, Ordering::Release);
}

fn create_fingerprint_watcher(
    spec: &FingerprintSpec,
    generation: Arc<AtomicU64>,
) -> Result<RecommendedWatcher> {
    let root = spec.root.clone();
    let include = build_glob_set(&spec.include, true)?;
    let exclude = build_glob_set(&spec.exclude, false)?;
    let callback_generation = Arc::clone(&generation);
    let mut watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
        let relevant = match result {
            Err(_) => true, // overflow/backend uncertainty: force a full rescan
            Ok(event) => {
                matches!(
                    event.kind,
                    EventKind::Create(_)
                        | EventKind::Modify(_)
                        | EventKind::Remove(_)
                        | EventKind::Any
                ) && event.paths.iter().any(|path| {
                    let relative = path.strip_prefix(root.as_path()).unwrap_or(path);
                    include.is_match(relative) && !exclude.is_match(relative)
                })
            }
        };
        if relevant {
            mark_fingerprint_dirty(&callback_generation);
        }
    })?;
    watcher.watch(spec.root.as_path(), RecursiveMode::Recursive)?;
    Ok(watcher)
}

fn compute_tree_fingerprint(root: &Path, include: &[&str], exclude: &[&str]) -> Result<String> {
    let files = zccache_fingerprint::walk_files_glob(root, include, exclude)
        .with_context(|| format!("scan fingerprint inputs under {}", root.display()))?;
    zccache_fingerprint::compute_aggregate_hash(&files)
        .with_context(|| format!("hash fingerprint inputs under {}", root.display()))
}

/// Hash a selected source tree with zccache's content-authoritative scanner.
/// Paths, file count, and bytes all participate, so additions, deletions, and
/// same-size edits with restored mtimes cannot produce a false cache hit.
pub(crate) fn fingerprint_tree(root: &Path, include: &[&str], exclude: &[&str]) -> Result<String> {
    if std::env::var_os("FASTLED_PERSISTENT_FINGERPRINTS").is_none() {
        return compute_tree_fingerprint(root, include, exclude);
    }

    let spec = FingerprintSpec {
        root: NormalizedPath::new(root),
        include: include.iter().map(|value| (*value).to_string()).collect(),
        exclude: exclude.iter().map(|value| (*value).to_string()).collect(),
    };
    let cache = FINGERPRINT_CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    let mut cache = cache
        .lock()
        .map_err(|_| anyhow::anyhow!("persistent fingerprint cache lock poisoned"))?;

    if let Some(entry) = cache.get_mut(&spec) {
        let current = entry.generation.load(Ordering::Acquire);
        if current == entry.observed_generation {
            return Ok(entry.value.clone());
        }
        // If writes continue during the scan, leave the generation stale so
        // the next build performs another authoritative scan.
        let before = current;
        let value = compute_tree_fingerprint(root, include, exclude)?;
        let after = entry.generation.load(Ordering::Acquire);
        entry.value = value.clone();
        entry.observed_generation = if before == after { after } else { before };
        return Ok(value);
    }

    let generation = Arc::new(AtomicU64::new(0));
    let watcher = match create_fingerprint_watcher(&spec, Arc::clone(&generation)) {
        Ok(watcher) => watcher,
        Err(_) => return compute_tree_fingerprint(root, include, exclude),
    };
    let before = generation.load(Ordering::Acquire);
    let value = compute_tree_fingerprint(root, include, exclude)?;
    let after = generation.load(Ordering::Acquire);
    cache.insert(
        spec,
        WatchedFingerprint {
            value: value.clone(),
            observed_generation: if before == after { after } else { before },
            generation,
            _watcher: watcher,
        },
    );
    Ok(value)
}

pub(crate) fn fingerprint_values<'a>(values: impl IntoIterator<Item = &'a [u8]>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"fastled-wasm-dynamic-cache-v1\0");
    for value in values {
        hasher.update((value.len() as u64).to_le_bytes());
        hasher.update(value);
    }
    format!("{:x}", hasher.finalize())
}

#[derive(Debug, Serialize, Deserialize)]
struct ArtifactRecord {
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheMetadata {
    schema: u32,
    fingerprint: String,
    artifacts: BTreeMap<String, ArtifactRecord>,
}

fn hash_file(path: &Path) -> Result<ArtifactRecord> {
    let mut file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("read {}", path.display()))?;
        if read == 0 {
            break;
        }
        bytes += read as u64;
        hasher.update(&buffer[..read]);
    }
    Ok(ArtifactRecord {
        bytes,
        sha256: format!("{:x}", hasher.finalize()),
    })
}

fn validate_artifact_shape(name: &str, path: &Path, bytes: u64) -> std::result::Result<(), String> {
    if bytes == 0 {
        return Err(format!("{name} is empty"));
    }
    if name.ends_with(".wasm") {
        let mut header = [0_u8; 8];
        File::open(path)
            .and_then(|mut file| file.read_exact(&mut header))
            .map_err(|err| format!("cannot read {name} header: {err}"))?;
        if header[..4] != *b"\0asm" || header[4..] != [1, 0, 0, 0] {
            return Err(format!(
                "{name} has an invalid WebAssembly magic or version"
            ));
        }
    }
    Ok(())
}

pub(crate) fn validate_entry(
    entry: &Path,
    fingerprint: &str,
    required: &[&str],
) -> std::result::Result<(), String> {
    let metadata_path = entry.join(METADATA_FILE);
    let source = fs::read_to_string(&metadata_path)
        .map_err(|err| format!("cannot read {}: {err}", metadata_path.display()))?;
    let metadata: CacheMetadata = serde_json::from_str(&source)
        .map_err(|err| format!("invalid {}: {err}", metadata_path.display()))?;
    if metadata.schema != CACHE_SCHEMA {
        return Err(format!(
            "cache schema mismatch: expected {CACHE_SCHEMA}, got {}",
            metadata.schema
        ));
    }
    if metadata.fingerprint != fingerprint {
        return Err("cache fingerprint mismatch".to_string());
    }
    for name in required {
        if !metadata.artifacts.contains_key(*name) {
            return Err(format!("metadata is missing required artifact {name}"));
        }
    }
    for (name, expected) in &metadata.artifacts {
        let path = entry.join(name);
        let actual = hash_file(&path).map_err(|err| err.to_string())?;
        validate_artifact_shape(name, &path, actual.bytes)?;
        if actual.bytes != expected.bytes || actual.sha256 != expected.sha256 {
            return Err(format!("artifact digest mismatch for {name}"));
        }
    }
    Ok(())
}

pub(crate) fn write_metadata(staging: &Path, fingerprint: &str, artifacts: &[&str]) -> Result<()> {
    let mut records = BTreeMap::new();
    for name in artifacts {
        let path = staging.join(name);
        let record = hash_file(&path)?;
        validate_artifact_shape(name, &path, record.bytes).map_err(anyhow::Error::msg)?;
        records.insert((*name).to_string(), record);
    }
    let metadata = CacheMetadata {
        schema: CACHE_SCHEMA,
        fingerprint: fingerprint.to_string(),
        artifacts: records,
    };
    fs::write(
        staging.join(METADATA_FILE),
        serde_json::to_vec_pretty(&metadata)?,
    )
    .with_context(|| format!("write cache metadata under {}", staging.display()))?;
    Ok(())
}

/// Publish a fully validated staging directory by one same-filesystem rename.
/// A key is never observable as successful until every artifact and its
/// metadata are complete.
pub(crate) fn publish_staging(staging: tempfile::TempDir, target: &Path) -> Result<()> {
    let staging_path = staging.keep();
    if target.exists() {
        fs::remove_dir_all(target)
            .with_context(|| format!("remove invalid cache entry {}", target.display()))?;
    }
    if let Err(error) = fs::rename(&staging_path, target) {
        fs::remove_dir_all(&staging_path).ok();
        return Err(error).with_context(|| {
            format!(
                "publish cache entry {} to {}",
                staging_path.display(),
                target.display()
            )
        });
    }
    Ok(())
}

pub(crate) fn staging_dir(cache_root: &Path, prefix: &str) -> Result<tempfile::TempDir> {
    fs::create_dir_all(cache_root)
        .with_context(|| format!("create cache root {}", cache_root.display()))?;
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(cache_root)
        .with_context(|| format!("create staging directory in {}", cache_root.display()))
}

pub(crate) struct CacheLock {
    _file: File,
}

impl CacheLock {
    pub(crate) fn acquire(cache_root: &Path, fingerprint: &str) -> Result<Self> {
        fs::create_dir_all(cache_root)
            .with_context(|| format!("create cache root {}", cache_root.display()))?;
        let path = cache_root.join(format!("{fingerprint}.lock"));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .with_context(|| format!("open cache lock {}", path.display()))?;
        fs2::FileExt::lock_exclusive(&file)
            .with_context(|| format!("lock cache key {fingerprint}"))?;
        Ok(Self { _file: file })
    }
}

pub(crate) fn entry_path(cache_root: &Path, fingerprint: &str) -> NormalizedPath {
    NormalizedPath::new(cache_root.join(fingerprint))
}

#[derive(Debug, Serialize, Deserialize)]
struct AttemptMetadata {
    status: String,
    phase: String,
    message: Option<String>,
}

fn attempt_path(cache_root: &Path, fingerprint: &str) -> NormalizedPath {
    NormalizedPath::new(cache_root.join(format!("{fingerprint}.attempt.json")))
}

pub(crate) fn previous_attempt(cache_root: &Path, fingerprint: &str) -> Option<String> {
    let path = attempt_path(cache_root, fingerprint);
    let source = fs::read_to_string(path).ok()?;
    let attempt: AttemptMetadata = serde_json::from_str(&source).ok()?;
    Some(match attempt.message {
        Some(message) => format!("{} {}: {message}", attempt.status, attempt.phase),
        None => format!("{} {}", attempt.status, attempt.phase),
    })
}

pub(crate) fn mark_pending(cache_root: &Path, fingerprint: &str, phase: &str) -> Result<()> {
    write_attempt(cache_root, fingerprint, "pending", phase, None)
}

pub(crate) fn mark_failure(
    cache_root: &Path,
    fingerprint: &str,
    phase: &str,
    error: &anyhow::Error,
) -> Result<()> {
    write_attempt(
        cache_root,
        fingerprint,
        "failure",
        phase,
        Some(format!("{error:#}")),
    )
}

fn write_attempt(
    cache_root: &Path,
    fingerprint: &str,
    status: &str,
    phase: &str,
    message: Option<String>,
) -> Result<()> {
    fs::create_dir_all(cache_root)?;
    let attempt = AttemptMetadata {
        status: status.to_string(),
        phase: phase.to_string(),
        message,
    };
    fs::write(
        attempt_path(cache_root, fingerprint),
        serde_json::to_vec_pretty(&attempt)?,
    )?;
    Ok(())
}

pub(crate) fn clear_attempt(cache_root: &Path, fingerprint: &str) -> Result<()> {
    let path = attempt_path(cache_root, fingerprint);
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use std::time::Duration;

    fn wasm_bytes(payload: &[u8]) -> Vec<u8> {
        let mut bytes = b"\0asm\x01\0\0\0".to_vec();
        bytes.extend_from_slice(payload);
        bytes
    }

    #[test]
    fn fingerprint_detects_same_size_edit_even_with_unchanged_mtime() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("sketch.ino");
        fs::write(&source, "aaaa").unwrap();
        let original_mtime = source.metadata().unwrap().modified().unwrap();
        let first = fingerprint_tree(temp.path(), &["**/*.ino"], &[]).unwrap();

        fs::write(&source, "bbbb").unwrap();
        let file = OpenOptions::new().write(true).open(&source).unwrap();
        file.set_modified(original_mtime).unwrap();
        let second = fingerprint_tree(temp.path(), &["**/*.ino"], &[]).unwrap();

        assert_ne!(first, second);
    }

    #[test]
    fn fingerprint_detects_source_deletion() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("a.cpp"), "a").unwrap();
        fs::write(temp.path().join("b.cpp"), "b").unwrap();
        let first = fingerprint_tree(temp.path(), &["**/*.cpp"], &[]).unwrap();
        fs::remove_file(temp.path().join("b.cpp")).unwrap();
        let second = fingerprint_tree(temp.path(), &["**/*.cpp"], &[]).unwrap();
        assert_ne!(first, second);
    }

    #[test]
    fn watcher_error_or_overflow_marks_fingerprint_dirty() {
        let generation = AtomicU64::new(7);
        mark_fingerprint_dirty(&generation);
        assert_eq!(generation.load(Ordering::Acquire), 8);
    }

    #[test]
    fn watcher_globs_match_selected_files_and_ignore_build_outputs() {
        let include = build_glob_set(&["emscripten/emcc.py".to_string()], false).unwrap();
        assert!(include.is_match(Path::new("emscripten/emcc.py")));
        if cfg!(windows) {
            assert!(include.is_match(Path::new(r"emscripten\emcc.py")));
        }
        let exclude = build_glob_set(&[".build/**".to_string()], false).unwrap();
        assert!(exclude.is_match(Path::new(".build/wasm/sketch.o")));
    }

    #[test]
    fn validation_rejects_missing_empty_truncated_and_corrupt_entries() {
        let temp = tempfile::tempdir().unwrap();
        let entry = temp.path().join("entry");
        fs::create_dir(&entry).unwrap();
        fs::write(entry.join("fastled.js"), "js").unwrap();
        fs::write(entry.join("fastled.wasm"), wasm_bytes(b"runtime")).unwrap();
        write_metadata(&entry, "key", &["fastled.js", "fastled.wasm"]).unwrap();
        assert!(validate_entry(&entry, "key", &["fastled.js", "fastled.wasm"]).is_ok());

        fs::remove_file(entry.join("fastled.js")).unwrap();
        assert!(validate_entry(&entry, "key", &["fastled.js", "fastled.wasm"]).is_err());
        fs::write(entry.join("fastled.js"), "").unwrap();
        assert!(validate_entry(&entry, "key", &["fastled.js", "fastled.wasm"]).is_err());
        fs::write(entry.join("fastled.js"), "js").unwrap();
        fs::write(entry.join("fastled.wasm"), b"\0asm").unwrap();
        assert!(validate_entry(&entry, "key", &["fastled.js", "fastled.wasm"]).is_err());
        fs::write(entry.join(METADATA_FILE), "not-json").unwrap();
        assert!(validate_entry(&entry, "key", &["fastled.js", "fastled.wasm"]).is_err());

        fs::write(entry.join("fastled.wasm"), b"\0asm\x02\0\0\0").unwrap();
        assert!(write_metadata(&entry, "key", &["fastled.js", "fastled.wasm"]).is_err());
    }

    #[test]
    fn cache_lock_serializes_same_key() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        let barrier = Arc::new(Barrier::new(2));
        let other_barrier = Arc::clone(&barrier);
        let handle = std::thread::spawn(move || {
            let _lock = CacheLock::acquire(&root, "same-key").unwrap();
            other_barrier.wait();
            std::thread::sleep(Duration::from_millis(150));
        });
        barrier.wait();
        let started = std::time::Instant::now();
        let _lock = CacheLock::acquire(temp.path(), "same-key").unwrap();
        assert!(started.elapsed() >= Duration::from_millis(100));
        handle.join().unwrap();
    }

    #[test]
    fn atomic_publish_replaces_invalid_entry() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("cache");
        fs::create_dir(&root).unwrap();
        let target = root.join("key");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("broken"), "old").unwrap();

        let staging = staging_dir(&root, ".staging-").unwrap();
        fs::write(staging.path().join("sketch.wasm"), wasm_bytes(b"side")).unwrap();
        write_metadata(staging.path(), "key", &["sketch.wasm"]).unwrap();
        publish_staging(staging, &target).unwrap();

        assert!(validate_entry(&target, "key", &["sketch.wasm"]).is_ok());
        assert!(!target.join("broken").exists());
    }

    #[test]
    fn attempt_state_records_pending_failure_and_clears_after_success() {
        let temp = tempfile::tempdir().unwrap();
        mark_pending(temp.path(), "key", "main-link").unwrap();
        assert_eq!(
            previous_attempt(temp.path(), "key").as_deref(),
            Some("pending main-link")
        );

        mark_failure(
            temp.path(),
            "key",
            "main-link",
            &anyhow::anyhow!("link failed"),
        )
        .unwrap();
        assert!(previous_attempt(temp.path(), "key")
            .unwrap()
            .contains("failure main-link: link failed"));

        clear_attempt(temp.path(), "key").unwrap();
        assert!(previous_attempt(temp.path(), "key").is_none());
    }
}
