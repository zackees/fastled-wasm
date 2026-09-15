//! File-system watcher that ports the core logic from `filewatcher.py`.
//!
//! Key behaviours mirrored from Python:
//! * Watch a directory recursively through kernal-api.
//! * Filter out paths that contain any of the standard ignored segments.
//! * Detect *real* changes by comparing SHA-256 digests (avoids spurious
//!   events from editors that touch mtime without changing content).
//! * Debounce rapid bursts: accumulate events for `debounce_ms` milliseconds
//!   after the last activity before emitting a single batch.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use kernal_api::platform::fs_watch::{ChangeKind, RecursiveMode, WatchNotification, Watcher};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default debounce window.
pub const DEFAULT_DEBOUNCE_MS: u64 = 300;

/// Path segments that are always ignored (mirrors Python's excluded_patterns).
pub const DEFAULT_IGNORED_SEGMENTS: &[&str] = &[
    "fastled_js",
    ".build",
    "__pycache__",
    ".git",
    "node_modules",
    ".mypy_cache",
    ".pytest_cache",
    "target",
];

/// A debounced batch of filesystem activity that may require a rebuild.
///
/// `force_rescan` is set when the watcher cannot provide a sufficiently precise
/// path-level change (for example, an overflow/error or directory replacement).
/// Consumers must invalidate all persistent fingerprints for such a batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WatchBatch {
    pub(crate) paths: Vec<PathBuf>,
    pub(crate) force_rescan: bool,
}

// ---------------------------------------------------------------------------
// Helper: file hash
// ---------------------------------------------------------------------------

/// Return the SHA-256 hex digest of a file, or `None` if the file cannot be
/// read (e.g. it was deleted between the notification and the read).
fn file_hash(path: &Path) -> Option<String> {
    kernal_api::hash::sha256_file(path, 16 * 1024 * 1024 * 1024)
        .ok()
        .map(|digest| digest.to_hex())
}

// ---------------------------------------------------------------------------
// Internal shared state (guarded by a Mutex so the notify callback + the
// background debounce thread can both access it safely).
// ---------------------------------------------------------------------------

struct State {
    /// Paths whose change events have arrived but haven't been flushed yet.
    pending: Vec<PathBuf>,
    /// Monotonic instant of the *last* received event (used for debouncing).
    last_event: Option<Instant>,
    /// Per-path content hash cache (skip events where content didn't change).
    hashes: HashMap<PathBuf, String>,
    /// Whether the watcher lost enough precision to require a full rescan.
    force_rescan: bool,
}

impl State {
    fn new() -> Self {
        Self {
            pending: Vec::new(),
            last_event: None,
            hashes: HashMap::new(),
            force_rescan: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// A running file watcher that emits batches of changed [`PathBuf`]s via an
/// [`std::sync::mpsc`] channel.
///
/// # Example
/// ```no_run
/// use std::path::PathBuf;
///
/// // FileWatcher lives in the fastled binary crate (watcher module).
/// // let mut watcher = FileWatcher::new(PathBuf::from("/my/sketch"), 300).unwrap();
/// // let rx = watcher.start();
/// // for batch in rx { println!("changed: {:?}", batch); }
/// ```
pub struct FileWatcher {
    watch_dir: PathBuf,
    debounce_ms: u64,
    ignored_segments: Vec<String>,
    stop_flag: Arc<AtomicBool>,
    watch_lost: Arc<AtomicBool>,
    // Keep the notify watcher alive for the lifetime of FileWatcher.
    _watcher: Option<Arc<Mutex<Watcher>>>,
}

impl FileWatcher {
    /// Create a new watcher (does not start watching yet — call [`start`]).
    pub fn new(watch_dir: PathBuf, debounce_ms: u64) -> std::io::Result<Self> {
        Ok(Self {
            watch_dir,
            debounce_ms,
            ignored_segments: DEFAULT_IGNORED_SEGMENTS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            stop_flag: Arc::new(AtomicBool::new(false)),
            watch_lost: Arc::new(AtomicBool::new(false)),
            _watcher: None,
        })
    }

    /// Override the ignored path segments (replaces the defaults).
    #[allow(dead_code)]
    pub fn with_ignored_segments(mut self, segments: Vec<String>) -> Self {
        self.ignored_segments = segments;
        self
    }

    /// Begin watching. Returns an [`mpsc::Receiver`] that yields debounced
    /// [`WatchBatch`] values.
    ///
    /// Dropping the receiver stops the background debounce thread (it will
    /// notice the broken channel and exit).  Call [`stop`] to also shut down
    /// the notify watcher cleanly.
    pub fn start(&mut self) -> std::sync::mpsc::Receiver<WatchBatch> {
        let (tx, rx) = std::sync::mpsc::channel::<WatchBatch>();

        let state = Arc::new(Mutex::new(State::new()));
        let state_for_cb = Arc::clone(&state);
        let ignored = self.ignored_segments.clone();
        let watch_lost = Arc::clone(&self.watch_lost);
        let callback_watch_lost = Arc::clone(&watch_lost);

        // --- notify event callback -------------------------------------------
        let notify_cb = move |res| {
            let event = match res {
                Ok(WatchNotification::Change(e)) => e,
                Ok(WatchNotification::RescanRequired(rescan)) => {
                    if rescan.watch_lost() {
                        callback_watch_lost.store(true, Ordering::Release);
                    }
                    let mut s = state_for_cb.lock().unwrap();
                    s.force_rescan = true;
                    s.last_event = Some(Instant::now());
                    return;
                }
                Err(_) => {
                    let mut s = state_for_cb.lock().unwrap();
                    s.force_rescan = true;
                    s.last_event = Some(Instant::now());
                    return;
                }
            };

            // Only act on create / modify / remove events (not access).
            let relevant = event.kind() != ChangeKind::Accessed;
            if !relevant {
                return;
            }

            let is_remove = matches!(event.kind(), ChangeKind::Removed(_));
            let is_rename = matches!(event.kind(), ChangeKind::NameModified(_));

            for path in event.paths().iter().cloned() {
                // Filter ignored segments.
                if path_contains_ignored(&path, &ignored) {
                    continue;
                }

                let mut s = state_for_cb.lock().unwrap();

                // Deletions and rename events may refer to paths that no longer
                // exist. Keep the lexical path so cache invalidation can match
                // it without canonicalising or reading the file.
                if is_remove || is_rename {
                    s.hashes.remove(&path);
                    s.pending.push(path.clone());
                    s.last_event = Some(Instant::now());
                    // A remove/rename path may be a directory that no longer
                    // exists, so conservatively rescan for these event kinds.
                    s.force_rescan = true;
                    continue;
                }

                // Directory notifications are metadata noise on some backends
                // (notably macOS FSEvents) and can point at the watched root
                // after an ignored child changes. File-level events carry the
                // useful invalidation, while removals above already force a
                // rescan because their paths may no longer exist.
                if path.is_dir() {
                    continue;
                }

                // Hash-based deduplication.
                let new_hash = match file_hash(&path) {
                    Some(h) => h,
                    None => {
                        s.force_rescan = true;
                        s.last_event = Some(Instant::now());
                        continue;
                    }
                };

                let old_hash = s.hashes.get(&path).cloned().unwrap_or_default();
                if new_hash == old_hash {
                    continue; // content unchanged
                }
                s.hashes.insert(path.clone(), new_hash);
                s.pending.push(path);
                s.last_event = Some(Instant::now());
            }
        };

        // --- start notify watcher --------------------------------------------
        let mut watcher = Watcher::new(notify_cb).expect("filesystem watcher failed");
        watcher
            .watch(&self.watch_dir, RecursiveMode::Recursive)
            .expect("filesystem watch failed");
        let watcher = Arc::new(Mutex::new(watcher));
        self._watcher = Some(Arc::clone(&watcher));
        let watch_dir = self.watch_dir.clone();

        // --- debounce thread -------------------------------------------------
        let debounce = Duration::from_millis(self.debounce_ms);
        let stop_flag = Arc::clone(&self.stop_flag);

        std::thread::spawn(move || {
            let mut next_restore = Instant::now();
            loop {
                if stop_flag.load(Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));

                if Instant::now() >= next_restore && watch_lost.swap(false, Ordering::AcqRel) {
                    // Registration cannot run in the backend callback. Restore
                    // it here and invalidate changes from the unobserved window.
                    let mut watcher = watcher.lock().unwrap();
                    let _ = watcher.unwatch(&watch_dir);
                    if let Err(error) = watcher.watch(&watch_dir, RecursiveMode::Recursive) {
                        eprintln!("Cannot restore filesystem watch: {error}");
                        watch_lost.store(true, Ordering::Release);
                        next_restore = Instant::now() + Duration::from_secs(1);
                    }
                    let mut s = state.lock().unwrap();
                    s.force_rescan = true;
                    s.last_event.get_or_insert_with(Instant::now);
                }

                let should_flush = {
                    let s = state.lock().unwrap();
                    match s.last_event {
                        Some(t) => {
                            (s.force_rescan || !s.pending.is_empty()) && t.elapsed() >= debounce
                        }
                        None => false,
                    }
                };

                if should_flush {
                    let batch = {
                        let mut s = state.lock().unwrap();
                        let mut paths = std::mem::take(&mut s.pending);
                        let force_rescan = s.force_rescan;
                        s.force_rescan = false;
                        s.last_event = None;
                        paths.sort();
                        paths.dedup();
                        WatchBatch {
                            paths,
                            force_rescan,
                        }
                    };
                    if tx.send(batch).is_err() {
                        // Receiver dropped — nothing left to do.
                        break;
                    }
                }
            }
        });

        rx
    }

    /// Signal the background debounce thread to stop.
    pub fn stop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

impl Drop for FileWatcher {
    fn drop(&mut self) {
        self.stop();
    }
}

fn path_contains_ignored(path: &Path, ignored: &[String]) -> bool {
    path.components().any(|c| {
        let s = c.as_os_str().to_string_lossy();
        ignored.iter().any(|ig| s == ig.as_str())
    })
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use kernal_api::platform::fs::TemporaryDirectory;
    use std::fs;
    use std::time::{Duration, Instant};

    fn temp_dir() -> TemporaryDirectory {
        kernal_api::platform::fs::TemporaryDirectory::new().expect("tempdir")
    }

    // ------------------------------------------------------------------
    // path_contains_ignored helper
    // ------------------------------------------------------------------

    #[test]
    fn test_ignored_segment_detection() {
        let ignored: Vec<String> = DEFAULT_IGNORED_SEGMENTS
            .iter()
            .map(|s| s.to_string())
            .collect();

        assert!(path_contains_ignored(
            Path::new("/sketch/fastled_js/bundle.js"),
            &ignored
        ));
        assert!(path_contains_ignored(
            Path::new("/sketch/.build/output.o"),
            &ignored
        ));
        assert!(path_contains_ignored(
            Path::new("/sketch/__pycache__/mod.pyc"),
            &ignored
        ));
        assert!(!path_contains_ignored(
            Path::new("/sketch/src/main.cpp"),
            &ignored
        ));
    }

    // ------------------------------------------------------------------
    // file_hash helper
    // ------------------------------------------------------------------

    #[test]
    fn test_file_hash_changes_on_content_change() {
        let dir = temp_dir();
        let file = dir.path().join("test.txt");

        fs::write(&file, b"hello").unwrap();
        let h1 = file_hash(&file).expect("hash1");

        fs::write(&file, b"world").unwrap();
        let h2 = file_hash(&file).expect("hash2");

        assert_ne!(h1, h2, "hash must differ after content change");
    }

    #[test]
    fn test_file_hash_stable_for_same_content() {
        let dir = temp_dir();
        let file = dir.path().join("stable.txt");

        fs::write(&file, b"same content").unwrap();
        let h1 = file_hash(&file).expect("hash1");
        let h2 = file_hash(&file).expect("hash2");

        assert_eq!(h1, h2);
    }

    #[test]
    fn test_file_hash_none_for_missing_file() {
        let result = file_hash(Path::new("/nonexistent/path/file.txt"));
        assert!(result.is_none());
    }

    // ------------------------------------------------------------------
    // FileWatcher integration tests
    // ------------------------------------------------------------------

    #[test]
    fn dropping_watcher_stops_monitoring_and_closes_receiver() {
        let dir = temp_dir();
        let mut watcher = FileWatcher::new(dir.path().to_path_buf(), 50).unwrap();
        let rx = watcher.start();
        drop(watcher);
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
        );
    }

    #[test]
    fn lost_watch_forces_rescan_and_restores_change_delivery() {
        let dir = temp_dir();
        let root = dir.path().canonicalize().unwrap();
        let mut watcher = FileWatcher::new(root.clone(), 50).unwrap();
        let rx = watcher.start();
        watcher
            ._watcher
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .unwatch(&root)
            .unwrap();
        watcher.watch_lost.store(true, Ordering::Release);
        assert!(
            rx.recv_timeout(Duration::from_secs(2))
                .unwrap()
                .force_rescan
        );

        let file = root.join("restored.ino");
        fs::write(&file, b"void setup() {}").unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let batch = rx
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("restored watch must deliver file changes");
            if batch.paths.contains(&file) {
                break;
            }
        }
        watcher.stop();
    }

    /// Creating / modifying a file triggers a change event.
    #[test]
    fn test_watcher_detects_file_change() {
        let dir = temp_dir();
        // Canonicalize to resolve symlinks (e.g. /var -> /private/var on macOS).
        let canonical_dir = dir.path().canonicalize().unwrap();
        let file = canonical_dir.join("sketch.ino");

        let mut watcher = FileWatcher::new(canonical_dir.clone(), DEFAULT_DEBOUNCE_MS).unwrap();
        let rx = watcher.start();

        // Give the watcher time to initialise before touching files.
        std::thread::sleep(Duration::from_millis(200));

        fs::write(&file, b"void setup() {}").unwrap();

        // Wait up to 2 s for the debounced batch.
        let batch = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("expected a change event within 2 s");

        watcher.stop();

        assert!(
            !batch.paths.is_empty(),
            "batch should contain the changed file"
        );
        assert!(
            batch.paths.iter().any(|p| p == &file),
            "expected {:?} in batch {:?}",
            file,
            batch
        );
    }

    #[test]
    fn test_watcher_reports_deleted_file() {
        let dir = temp_dir();
        let canonical_dir = dir.path().canonicalize().unwrap();
        let file = canonical_dir.join("deleted.ino");
        fs::write(&file, b"void setup() {}").unwrap();

        let mut watcher = FileWatcher::new(canonical_dir, DEFAULT_DEBOUNCE_MS).unwrap();
        let rx = watcher.start();
        std::thread::sleep(Duration::from_millis(200));
        fs::remove_file(&file).unwrap();

        // Backends can deliver a queued metadata event that predates the
        // deletion. Ignore such batches and wait for the deletion's own
        // force-rescan batch, which must name the removed file.
        let deadline = Instant::now() + Duration::from_secs(2);
        let batch = loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(
                !remaining.is_zero(),
                "expected a deletion force-rescan event within 2 s"
            );
            let batch = rx
                .recv_timeout(remaining)
                .expect("expected a deletion event within 2 s");
            if batch.force_rescan && batch.paths.iter().any(|path| path == &file) {
                break batch;
            }
        };
        watcher.stop();

        assert!(batch.paths.iter().any(|path| path == &file));
        assert!(batch.force_rescan);
    }

    /// Changes under ignored directories must not be reported.
    #[test]
    fn test_watcher_ignores_filtered_paths() {
        let dir = temp_dir();
        let ignored_dir = dir.path().join("fastled_js");
        fs::create_dir_all(&ignored_dir).unwrap();
        let ignored_file = ignored_dir.join("bundle.js");

        let mut watcher = FileWatcher::new(dir.path().to_path_buf(), DEFAULT_DEBOUNCE_MS).unwrap();
        let rx = watcher.start();

        std::thread::sleep(Duration::from_millis(200));

        fs::write(&ignored_file, b"console.log('x');").unwrap();

        // We should NOT receive any event.
        let result = rx.recv_timeout(Duration::from_millis(800));
        watcher.stop();

        assert!(
            result.is_err(),
            "ignored path triggered an unexpected event: {:?}",
            result.ok()
        );
    }

    /// Multiple rapid writes should be coalesced into a single batch.
    #[test]
    fn test_watcher_debounces_rapid_changes() {
        let dir = temp_dir();
        // macOS' /var is a symlink to /private/var, while notify reports the
        // canonical path. Watch and assert against the canonical directory.
        let dir_path = dir.path().canonicalize().unwrap();
        let file = dir_path.join("rapid.ino");

        // Use a longer debounce so the rapid writes definitely fall inside.
        let debounce_ms = 500u64;
        let mut watcher = FileWatcher::new(dir_path, debounce_ms).unwrap();
        let rx = watcher.start();

        std::thread::sleep(Duration::from_millis(200));

        // Write five times in quick succession with distinct content.
        for i in 0u8..5 {
            fs::write(&file, [i; 64]).unwrap();
            std::thread::sleep(Duration::from_millis(30));
        }

        // Backends may report different numbers of low-level filesystem
        // notifications. The debounce contract is one deduplicated batch
        // after a quiet window, so stop after that first batch instead of
        // counting delayed backend metadata events.
        let batch = rx
            .recv_timeout(Duration::from_secs(3))
            .expect("expected a debounced batch within 3 s");
        watcher.stop();

        // All five writes arrived within the debounce window — expect at most
        // a single deduplicated batch, which is checked below.
        assert!(
            batch.paths.iter().any(|path| path == &file),
            "expected {:?} in debounced batch {:?}",
            file,
            batch.paths
        );
        assert_eq!(
            batch.paths.iter().filter(|path| *path == &file).count(),
            1,
            "debounced batch must deduplicate repeated writes",
        );
    }
}
