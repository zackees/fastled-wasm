use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::error_compat::{Context, Result};
use kernal_api::hash::Sha256Hasher as Sha256;
use kernal_api::json::{self, Layout, Value};
use kernal_api::platform::fs::{PatternSet, PatternSetBuilder};
use kernal_api::platform::fs_watch::{ChangeKind, RecursiveMode, WatchNotification, Watcher};

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
    watch_lost: Arc<AtomicBool>,
    _watcher: Watcher,
}

static FINGERPRINT_CACHE: OnceLock<
    Mutex<std::collections::HashMap<FingerprintSpec, WatchedFingerprint>>,
> = OnceLock::new();

fn build_glob_set(patterns: &[String], default_all: bool) -> Result<PatternSet> {
    let mut builder = PatternSetBuilder::new();
    if patterns.is_empty() && default_all {
        builder = builder.add_pattern("**/*");
    } else {
        for pattern in patterns {
            builder = builder.add_pattern(pattern);
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
    watch_lost: Arc<AtomicBool>,
) -> Result<Watcher> {
    let root = spec.root.clone();
    let include = build_glob_set(&spec.include, true)?;
    let exclude = build_glob_set(&spec.exclude, false)?;
    let callback_generation = Arc::clone(&generation);
    let mut watcher = Watcher::new(move |result| {
        let relevant = match result {
            Err(_) => true, // overflow/backend uncertainty: force a full rescan
            Ok(WatchNotification::RescanRequired(rescan)) => {
                if rescan.watch_lost() {
                    watch_lost.store(true, Ordering::Release);
                }
                true
            }
            Ok(WatchNotification::Change(event)) => {
                event.kind() != ChangeKind::Accessed
                    && event.paths().iter().any(|path| {
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
    kernal_api::hash::blake3_tree(root, include, exclude, Default::default())
        .map(|digest| digest.to_hex())
        .with_context(|| format!("hash fingerprint inputs under {}", root.display()))
}

/// Hash a selected source tree with the kernel's content-authoritative scanner.
/// Paths, file count, and bytes all participate, so additions, deletions, and
/// same-size edits with restored mtimes cannot produce a false cache hit.
pub(crate) fn fingerprint_tree(root: &Path, include: &[&str], exclude: &[&str]) -> Result<String> {
    if std::env::var_os("FASTLED_PERSISTENT_FINGERPRINTS").is_none() {
        return compute_tree_fingerprint(root, include, exclude);
    }
    fingerprint_tree_persistent(root, include, exclude)
}

fn fingerprint_tree_persistent(root: &Path, include: &[&str], exclude: &[&str]) -> Result<String> {
    let spec = FingerprintSpec {
        root: NormalizedPath::new(root),
        include: include.iter().map(|value| (*value).to_string()).collect(),
        exclude: exclude.iter().map(|value| (*value).to_string()).collect(),
    };
    let cache = FINGERPRINT_CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    let mut cache = cache
        .lock()
        .map_err(|_| crate::error_compat::error!("persistent fingerprint cache lock poisoned"))?;

    // Recreate dead watchers before trusting a cached value. If registration
    // fails, the normal construction path below performs an authoritative scan.
    if cache
        .get(&spec)
        .is_some_and(|entry| entry.watch_lost.load(Ordering::Acquire))
    {
        cache.remove(&spec);
    }

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
    let watch_lost = Arc::new(AtomicBool::new(false));
    let watcher =
        match create_fingerprint_watcher(&spec, Arc::clone(&generation), Arc::clone(&watch_lost)) {
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
            watch_lost,
            _watcher: watcher,
        },
    );
    Ok(value)
}

fn fingerprint_spec_matches_path(spec: &FingerprintSpec, path: &Path) -> Result<bool> {
    let path = NormalizedPath::new(path);
    let root = spec.root.as_path();
    if path.as_path() == root {
        return Ok(true);
    }
    let Ok(relative) = path.as_path().strip_prefix(root) else {
        return Ok(false);
    };
    let include = build_glob_set(&spec.include, true)?;
    let exclude = build_glob_set(&spec.exclude, false)?;
    Ok(include.is_match(relative) && !exclude.is_match(relative))
}

/// Mark cached fingerprints whose input set contains one of `paths` as dirty.
///
/// This is deliberately synchronous with the rebuild-triggering watcher batch;
/// the per-fingerprint filesystem watchers remain only as a secondary safety
/// net for changes that happen during a scan.
pub(crate) fn invalidate_persistent_fingerprints(paths: &[NormalizedPath]) -> Result<usize> {
    let Some(cache) = FINGERPRINT_CACHE.get() else {
        return Ok(0);
    };
    let cache = cache
        .lock()
        .map_err(|_| crate::error_compat::error!("persistent fingerprint cache lock poisoned"))?;
    let mut invalidated = 0;
    for (spec, entry) in cache.iter() {
        if paths
            .iter()
            .any(|path| fingerprint_spec_matches_path(spec, path).unwrap_or(true))
        {
            mark_fingerprint_dirty(&entry.generation);
            invalidated += 1;
        }
    }
    Ok(invalidated)
}

/// Mark every persistent fingerprint as dirty before a manual or uncertain rebuild.
pub(crate) fn invalidate_all_persistent_fingerprints() -> Result<usize> {
    let Some(cache) = FINGERPRINT_CACHE.get() else {
        return Ok(0);
    };
    let cache = cache
        .lock()
        .map_err(|_| crate::error_compat::error!("persistent fingerprint cache lock poisoned"))?;
    for entry in cache.values() {
        mark_fingerprint_dirty(&entry.generation);
    }
    Ok(cache.len())
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

#[derive(Debug)]
struct ArtifactRecord {
    bytes: u64,
    sha256: String,
}

#[derive(Debug)]
struct CacheMetadata {
    schema: u32,
    fingerprint: String,
    artifacts: BTreeMap<String, ArtifactRecord>,
}

// Cache schema adapters: unknown fields are ignored, known fields may not be
// repeated, and positional records require every declared slot.
fn cache_fields<const N: usize>(value: Value, names: [&str; N]) -> Result<[Option<Value>; N]> {
    let mut fields = std::array::from_fn(|_| None);
    match value {
        Value::ObjectMembers(members) => {
            for (name, value) in members {
                if let Some(index) = names.iter().position(|field| *field == name) {
                    if fields[index].replace(value).is_some() {
                        crate::error_compat::bail!(
                            "duplicate cache metadata field {}",
                            names[index]
                        );
                    }
                }
            }
        }
        Value::Array(values) if values.len() == N => {
            for (field, value) in fields.iter_mut().zip(values) {
                *field = Some(value);
            }
        }
        _ => crate::error_compat::bail!("invalid cache metadata record"),
    }
    Ok(fields)
}

fn cache_string(value: Option<Value>, name: &str) -> Result<String> {
    let Some(Value::String(value)) = value else {
        crate::error_compat::bail!("cache metadata {name} must be a string");
    };
    Ok(value)
}

fn cache_unsigned(value: Option<Value>, name: &str) -> Result<u64> {
    match value {
        Some(Value::Unsigned(value)) => Ok(value),
        Some(Value::Signed(value)) if value >= 0 => Ok(value as u64),
        _ => crate::error_compat::bail!("cache metadata {name} must be an unsigned integer"),
    }
}

impl CacheMetadata {
    fn parse(source: &str) -> Result<Self> {
        let [schema, fingerprint, artifacts] = cache_fields(
            json::parse_members(source.as_bytes())?,
            ["schema", "fingerprint", "artifacts"],
        )?;
        let schema = u32::try_from(cache_unsigned(schema, "schema")?)?;
        let fingerprint = cache_string(fingerprint, "fingerprint")?;
        let Some(Value::ObjectMembers(members)) = artifacts else {
            crate::error_compat::bail!("cache metadata artifacts must be an object");
        };
        let mut artifacts = BTreeMap::new();
        for (name, value) in members {
            // Validate every record before replacing a repeated map entry.
            let [bytes, sha256] = cache_fields(value, ["bytes", "sha256"])?;
            artifacts.insert(
                name,
                ArtifactRecord {
                    bytes: cache_unsigned(bytes, "bytes")?,
                    sha256: cache_string(sha256, "sha256")?,
                },
            );
        }
        Ok(Self {
            schema,
            fingerprint,
            artifacts,
        })
    }

    fn document(&self) -> Value {
        let artifacts = self
            .artifacts
            .iter()
            .map(|(name, record)| {
                (
                    name.clone(),
                    Value::ObjectMembers(vec![
                        ("bytes".into(), Value::Unsigned(record.bytes)),
                        ("sha256".into(), Value::String(record.sha256.clone())),
                    ]),
                )
            })
            .collect();
        Value::ObjectMembers(vec![
            ("schema".into(), Value::Unsigned(self.schema.into())),
            (
                "fingerprint".into(),
                Value::String(self.fingerprint.clone()),
            ),
            ("artifacts".into(), Value::Object(artifacts)),
        ])
    }
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
    let metadata = CacheMetadata::parse(&source)
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
        validate_artifact_shape(name, &path, record.bytes).map_err(crate::error_compat::message)?;
        records.insert((*name).to_string(), record);
    }
    let metadata = CacheMetadata {
        schema: CACHE_SCHEMA,
        fingerprint: fingerprint.to_string(),
        artifacts: records,
    };
    fs::write(
        staging.join(METADATA_FILE),
        json::encode(&metadata.document(), Layout::Pretty)?,
    )
    .with_context(|| format!("write cache metadata under {}", staging.display()))?;
    Ok(())
}

/// Publish a fully validated staging directory by one same-filesystem rename.
/// A key is never observable as successful until every artifact and its
/// metadata are complete.
pub(crate) fn publish_staging(
    staging: kernal_api::platform::fs::TemporaryDirectory,
    target: &Path,
) -> Result<()> {
    let staging_path = staging.persist();
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

pub(crate) fn staging_dir(
    cache_root: &Path,
    prefix: &str,
) -> Result<kernal_api::platform::fs::TemporaryDirectory> {
    fs::create_dir_all(cache_root)
        .with_context(|| format!("create cache root {}", cache_root.display()))?;
    kernal_api::platform::fs::TemporaryDirectory::in_directory(cache_root, prefix)
        .with_context(|| format!("create staging directory in {}", cache_root.display()))
}

pub(crate) struct CacheLock {
    _lock: kernal_api::platform::fs::OwnedFileLock,
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
        let lock = kernal_api::platform::fs::lock_exclusive_owned(file)
            .with_context(|| format!("lock cache key {fingerprint}"))?;
        Ok(Self { _lock: lock })
    }
}

pub(crate) fn entry_path(cache_root: &Path, fingerprint: &str) -> NormalizedPath {
    NormalizedPath::new(cache_root.join(fingerprint))
}

#[derive(Debug)]
struct AttemptMetadata {
    status: String,
    phase: String,
    message: Option<String>,
}

impl AttemptMetadata {
    fn parse(source: &str) -> Result<Self> {
        let [status, phase, message] = cache_fields(
            json::parse_members(source.as_bytes())?,
            ["status", "phase", "message"],
        )?;
        let message = match message {
            None | Some(Value::Null) => None,
            value => Some(cache_string(value, "message")?),
        };
        Ok(Self {
            status: cache_string(status, "status")?,
            phase: cache_string(phase, "phase")?,
            message,
        })
    }

    fn document(&self) -> Value {
        Value::ObjectMembers(vec![
            ("status".into(), Value::String(self.status.clone())),
            ("phase".into(), Value::String(self.phase.clone())),
            (
                "message".into(),
                self.message
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            ),
        ])
    }
}

fn attempt_path(cache_root: &Path, fingerprint: &str) -> NormalizedPath {
    NormalizedPath::new(cache_root.join(format!("{fingerprint}.attempt.json")))
}

pub(crate) fn previous_attempt(cache_root: &Path, fingerprint: &str) -> Option<String> {
    let path = attempt_path(cache_root, fingerprint);
    let source = fs::read_to_string(path).ok()?;
    let attempt = AttemptMetadata::parse(&source).ok()?;
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
    error: &crate::error_compat::Error,
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
        json::encode(&attempt.document(), Layout::Pretty)?,
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
    fn sha256_cache_encoding_preserves_domain_lengths_and_leading_zeroes() {
        // Fixed legacy encodings, independently checked with Python hashlib.
        // These protect FastLED's cache protocol; generic SHA vectors live upstream.
        for (values, expected) in [
            (
                vec![],
                "a176241cca24eb86c1fc3b441c63aa8d37d409f5821163583f3ce7320e625e81",
            ),
            (
                vec![&b"ab"[..], &b"c"[..]],
                "0d9a687b8e558f37b5d34c32b8a27fdd0c34b90e2313c922b977a1fbfd8c2979",
            ),
            (
                vec![&b"a"[..], &b"bc"[..]],
                "8cf4aafa0e399335648f6c2dc69f36b88f4608dfecc89dbe37fdd8e2d1c3d7ca",
            ),
            (
                vec![&b""[..], &b"ab"[..], &b"c"[..]],
                "fe43050a4dd2a7da0ae131b5c170ed024abc0544f71ea44e1778ff9f8de8c36a",
            ),
        ] {
            assert_eq!(fingerprint_values(values), expected);
        }
    }

    #[test]
    fn sha256_artifact_record_preserves_serialized_encoding() {
        let temp = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        let path = temp.path().join("firmware.wasm");
        fs::write(&path, wasm_bytes(b"")).unwrap();
        let metadata = CacheMetadata {
            schema: 1,
            fingerprint: "key".into(),
            artifacts: BTreeMap::from([("firmware.wasm".into(), hash_file(&path).unwrap())]),
        };
        assert_eq!(
            json::encode(&metadata.document(), Layout::Compact).unwrap(),
            br#"{"schema":1,"fingerprint":"key","artifacts":{"firmware.wasm":{"bytes":8,"sha256":"93a44bbb96c751218e4c00d479e4c14358122a389acca16205b1e4d0dc5f9476"}}}"#
        );
    }

    // Regression coverage for #193: the rebuild-triggering event must dirty
    // the persistent fingerprint before the next lookup.
    #[test]
    fn explicit_invalidation_detects_immediate_same_size_edit_with_restored_mtime() {
        let temp = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        let source = temp.path().join("sketch.ino");
        fs::write(&source, "aaaa").unwrap();
        let mut fingerprint = fingerprint_tree_persistent(temp.path(), &["**/*.ino"], &[]).unwrap();
        let normalized_source = NormalizedPath::new(&source);
        let mut expected = "bbbb";
        for _ in 0..1000 {
            let original_mtime = source.metadata().unwrap().modified().unwrap();
            fs::write(&source, expected).unwrap();
            let file = OpenOptions::new().write(true).open(&source).unwrap();
            file.set_modified(original_mtime).unwrap();
            assert_eq!(
                invalidate_persistent_fingerprints(std::slice::from_ref(&normalized_source))
                    .unwrap(),
                1
            );
            let next_fingerprint =
                fingerprint_tree_persistent(temp.path(), &["**/*.ino"], &[]).unwrap();
            assert_ne!(fingerprint, next_fingerprint);
            fingerprint = next_fingerprint;
            expected = if expected == "bbbb" { "aaaa" } else { "bbbb" };
        }
    }

    #[test]
    fn path_invalidation_dirties_only_matching_spec() {
        let first = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        let second = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        fs::write(first.path().join("one.cpp"), "one").unwrap();
        fs::write(second.path().join("two.cpp"), "two").unwrap();
        fingerprint_tree_persistent(first.path(), &["**/*.cpp"], &[]).unwrap();
        fingerprint_tree_persistent(second.path(), &["**/*.cpp"], &[]).unwrap();
        assert_eq!(
            invalidate_persistent_fingerprints(&[NormalizedPath::new(
                first.path().join("one.cpp")
            )])
            .unwrap(),
            1
        );
    }

    #[test]
    fn lost_fingerprint_watch_discards_cached_value_and_registers_again() {
        let temp = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        fs::write(temp.path().join("sketch.ino"), "aaaa").unwrap();
        let expected = fingerprint_tree_persistent(temp.path(), &["**/*.ino"], &[]).unwrap();
        let spec = FingerprintSpec {
            root: NormalizedPath::new(temp.path()),
            include: vec!["**/*.ino".to_owned()],
            exclude: vec![],
        };
        {
            let mut cache = FINGERPRINT_CACHE.get().unwrap().lock().unwrap();
            let entry = cache.get_mut(&spec).unwrap();
            entry._watcher.unwatch(temp.path()).unwrap();
            entry.value = "stale value from before the lost watch".to_owned();
            entry.observed_generation = entry.generation.load(Ordering::Acquire);
            entry.watch_lost.store(true, Ordering::Release);
        }
        assert_eq!(
            fingerprint_tree_persistent(temp.path(), &["**/*.ino"], &[]).unwrap(),
            expected
        );
        let cache = FINGERPRINT_CACHE.get().unwrap().lock().unwrap();
        assert!(!cache.get(&spec).unwrap().watch_lost.load(Ordering::Acquire));
    }

    #[test]
    fn invalidate_all_dirties_every_spec() {
        let first = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        let second = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        fs::write(first.path().join("one.cpp"), "one").unwrap();
        fs::write(second.path().join("two.cpp"), "two").unwrap();
        fingerprint_tree_persistent(first.path(), &["**/*.cpp"], &[]).unwrap();
        fingerprint_tree_persistent(second.path(), &["**/*.cpp"], &[]).unwrap();

        assert!(invalidate_all_persistent_fingerprints().unwrap() >= 2);
    }

    #[test]
    fn excluded_output_path_does_not_dirty_spec() {
        let temp = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        fs::create_dir_all(temp.path().join("fastled_js")).unwrap();
        fs::write(temp.path().join("source.cpp"), "source").unwrap();
        fingerprint_tree_persistent(temp.path(), &["**/*.cpp"], &["fastled_js/**"]).unwrap();
        fs::write(temp.path().join("fastled_js/bundle.cpp"), "output").unwrap();
        assert_eq!(
            invalidate_persistent_fingerprints(&[NormalizedPath::new(
                temp.path().join("fastled_js/bundle.cpp"),
            )])
            .unwrap(),
            0
        );
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
        let temp = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
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
    fn cache_json_schema_preserves_record_and_artifact_map_rules() {
        let temp = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        fs::write(temp.path().join("a.js"), "js").unwrap();
        let digest = hash_file(&temp.path().join("a.js")).unwrap().sha256;
        let record = format!(r#"{{"bytes":2,"sha256":"{digest}"}}"#);
        for source in [
            format!(
                r#"{{"schema":1,"fingerprint":"key","artifacts":{{"a.js":{record}}},"future":1,"future":2}}"#
            ),
            format!(r#"[1,"key",{{"a.js":[2,"{digest}"]}}]"#),
            // Map keys keep their last valid record, unlike known struct fields.
            format!(
                r#"{{"schema":1,"fingerprint":"key","artifacts":{{"a.js":{{"bytes":1,"sha256":"old"}},"a.js":{record}}}}}"#
            ),
        ] {
            fs::write(temp.path().join(METADATA_FILE), source).unwrap();
            assert!(validate_entry(temp.path(), "key", &["a.js"]).is_ok());
        }
        for source in [
            format!(
                r#"{{"schema":1,"schema":1,"fingerprint":"key","artifacts":{{"a.js":{record}}}}}"#
            ),
            format!(r#"{{"schema":1.0,"fingerprint":"key","artifacts":{{"a.js":{record}}}}}"#),
            format!(
                r#"{{"schema":1,"fingerprint":"key","artifacts":{{"a.js":{{"bytes":2,"bytes":2,"sha256":"{digest}"}}}}}}"#
            ),
            format!(
                r#"{{"schema":1,"fingerprint":"key","artifacts":{{"a.js":{{"bytes":null}},"a.js":{record}}}}}"#
            ),
        ] {
            fs::write(temp.path().join(METADATA_FILE), source).unwrap();
            assert!(validate_entry(temp.path(), "key", &["a.js"]).is_err());
        }
    }

    #[test]
    fn attempt_json_schema_preserves_nulls_duplicates_and_positional_records() {
        let temp = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        for (source, expected) in [
            (
                r#"{"status":"pending","phase":"日本"}"#,
                Some("pending 日本"),
            ),
            (
                r#"{"status":"pending","phase":"日本","message":null}"#,
                Some("pending 日本"),
            ),
            (
                r#"["failure","link","failed"]"#,
                Some("failure link: failed"),
            ),
            (r#"["pending","link"]"#, None),
            (
                r#"{"status":"pending","phase":"link","message":null,"message":null}"#,
                None,
            ),
            (
                r#"{"status":"pending","phase":"link","message":false}"#,
                None,
            ),
        ] {
            fs::write(attempt_path(temp.path(), "key"), source).unwrap();
            assert_eq!(previous_attempt(temp.path(), "key").as_deref(), expected);
        }
    }

    #[test]
    fn cache_json_preserves_unsigned_ranges_and_ordered_output() {
        let metadata =
            CacheMetadata::parse(r#"[4294967295,"key",{"a.js":[18446744073709551615,"digest"]}]"#)
                .unwrap();
        assert_eq!(metadata.schema, u32::MAX);
        assert_eq!(metadata.artifacts["a.js"].bytes, u64::MAX);
        for source in [
            r#"[4294967296,"key",{}]"#,
            r#"[1,"key",{"a.js":[-1,"digest"]}]"#,
            r#"[1,"key",{"a.js":[1.0,"digest"]}]"#,
            r#"[1,"key",{"a.js":[18446744073709551616,"digest"]}]"#,
        ] {
            assert!(CacheMetadata::parse(source).is_err(), "accepted {source}");
        }
        let metadata = CacheMetadata::parse(r#"[1,"key",{"b":[2,"b"],"a":[1,"a"]}]"#).unwrap();
        assert_eq!(json::encode(&metadata.document(), Layout::Compact).unwrap(), br#"{"schema":1,"fingerprint":"key","artifacts":{"a":{"bytes":1,"sha256":"a"},"b":{"bytes":2,"sha256":"b"}}}"#);
        let temp = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        mark_pending(temp.path(), "key", "link").unwrap();
        assert_eq!(
            fs::read_to_string(attempt_path(temp.path(), "key")).unwrap(),
            "{\n  \"status\": \"pending\",\n  \"phase\": \"link\",\n  \"message\": null\n}"
        );
    }

    #[test]
    fn cache_lock_serializes_same_key() {
        let temp = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
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
        let temp = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
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
        let temp = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        mark_pending(temp.path(), "key", "main-link").unwrap();
        assert_eq!(
            previous_attempt(temp.path(), "key").as_deref(),
            Some("pending main-link")
        );

        mark_failure(
            temp.path(),
            "key",
            "main-link",
            &crate::error_compat::error!("link failed"),
        )
        .unwrap();
        assert!(previous_attempt(temp.path(), "key")
            .unwrap()
            .contains("failure main-link: link failed"));

        clear_attempt(temp.path(), "key").unwrap();
        assert!(previous_attempt(temp.path(), "key").is_none());
    }
}
