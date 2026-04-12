//! File-system watcher that ports the core logic from `filewatcher.py`.
//!
//! Key behaviours mirrored from Python:
//! * Watch a directory recursively via the [`notify`] crate.
//! * Filter out paths that contain any of the standard ignored segments.
//! * Detect *real* changes by comparing SHA-256 digests (avoids spurious
//!   events from editors that touch mtime without changing content).
//! * Debounce rapid bursts: accumulate events for `debounce_ms` milliseconds
//!   after the last activity before emitting a single batch.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use notify::{
    event::{ModifyKind, RenameMode},
    Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
};
use sha2::{Digest, Sha256};

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

// ---------------------------------------------------------------------------
// Helper: file hash
// ---------------------------------------------------------------------------

/// Return the SHA-256 hex digest of a file, or `None` if the file cannot be
/// read (e.g. it was deleted between the notification and the read).
fn file_hash(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let digest = Sha256::digest(&bytes);
    Some(format!("{digest:x}"))
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
}

impl State {
    fn new() -> Self {
        Self {
            pending: Vec::new(),
            last_event: None,
            hashes: HashMap::new(),
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
    // Keep the notify watcher alive for the lifetime of FileWatcher.
    _watcher: Option<RecommendedWatcher>,
}

impl FileWatcher {
    /// Create a new watcher (does not start watching yet — call [`start`]).
    pub fn new(watch_dir: PathBuf, debounce_ms: u64) -> Result<Self, notify::Error> {
        Ok(Self {
            watch_dir,
            debounce_ms,
            ignored_segments: DEFAULT_IGNORED_SEGMENTS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            stop_flag: Arc::new(AtomicBool::new(false)),
            _watcher: None,
        })
    }

    /// Override the ignored path segments (replaces the defaults).
    #[allow(dead_code)]
    pub fn with_ignored_segments(mut self, segments: Vec<String>) -> Self {
        self.ignored_segments = segments;
        self
    }

    /// Begin watching.  Returns an [`mpsc::Receiver`] that yields `Vec<PathBuf>`
    /// batches after each debounce window expires.
    ///
    /// Dropping the receiver stops the background debounce thread (it will
    /// notice the broken channel and exit).  Call [`stop`] to also shut down
    /// the notify watcher cleanly.
    pub fn start(&mut self) -> std::sync::mpsc::Receiver<Vec<PathBuf>> {
        let (tx, rx) = std::sync::mpsc::channel::<Vec<PathBuf>>();

        let state = Arc::new(Mutex::new(State::new()));
        let state_for_cb = Arc::clone(&state);
        let ignored = self.ignored_segments.clone();

        // --- notify event callback -------------------------------------------
        let notify_cb = move |res: notify::Result<Event>| {
            let event = match res {
                Ok(e) => e,
                Err(_) => return,
            };

            // Only act on create / modify / rename events (not access).
            let relevant = matches!(
                event.kind,
                EventKind::Create(_)
                    | EventKind::Modify(ModifyKind::Data(_))
                    | EventKind::Modify(ModifyKind::Name(RenameMode::To))
                    | EventKind::Modify(ModifyKind::Any)
                    | EventKind::Remove(_)
            );
            if !relevant {
                return;
            }

            for path in event.paths {
                // Directories are skipped (we only care about file content).
                if path.is_dir() {
                    continue;
                }
                // Filter ignored segments.
                if path_contains_ignored(&path, &ignored) {
                    continue;
                }

                // Hash-based deduplication.
                let new_hash = match file_hash(&path) {
                    Some(h) => h,
                    None => continue, // deleted or unreadable
                };

                let mut s = state_for_cb.lock().unwrap();
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
        let mut watcher =
            RecommendedWatcher::new(notify_cb, Config::default()).expect("notify watcher failed");
        watcher
            .watch(&self.watch_dir, RecursiveMode::Recursive)
            .expect("notify watch failed");
        self._watcher = Some(watcher);

        // --- debounce thread -------------------------------------------------
        let debounce = Duration::from_millis(self.debounce_ms);
        let stop_flag = Arc::clone(&self.stop_flag);

        std::thread::spawn(move || {
            loop {
                if stop_flag.load(Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));

                let should_flush = {
                    let s = state.lock().unwrap();
                    match s.last_event {
                        Some(t) => !s.pending.is_empty() && t.elapsed() >= debounce,
                        None => false,
                    }
                };

                if should_flush {
                    let batch = {
                        let mut s = state.lock().unwrap();
                        let mut batch: Vec<PathBuf> = s.pending.drain(..).collect();
                        s.last_event = None;
                        batch.sort();
                        batch.dedup();
                        batch
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
    use std::fs;
    use std::time::Duration;
    use tempfile::TempDir;

    fn temp_dir() -> TempDir {
        tempfile::tempdir().expect("tempdir")
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

        assert!(!batch.is_empty(), "batch should contain the changed file");
        assert!(
            batch.iter().any(|p| p == &file),
            "expected {:?} in batch {:?}",
            file,
            batch
        );
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
        let file = dir.path().join("rapid.ino");

        // Use a longer debounce so the rapid writes definitely fall inside.
        let debounce_ms = 500u64;
        let mut watcher = FileWatcher::new(dir.path().to_path_buf(), debounce_ms).unwrap();
        let rx = watcher.start();

        std::thread::sleep(Duration::from_millis(200));

        // Write five times in quick succession with distinct content.
        for i in 0u8..5 {
            fs::write(&file, [i; 64]).unwrap();
            std::thread::sleep(Duration::from_millis(30));
        }

        // Collect all batches that arrive within a 2 s window.
        let mut batches: Vec<Vec<PathBuf>> = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(2);
        while let Ok(batch) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            batches.push(batch);
            if Instant::now() >= deadline {
                break;
            }
        }

        watcher.stop();

        // All five writes arrived within the debounce window — expect at most
        // a small number of batches (ideally 1).
        assert!(!batches.is_empty(), "expected at least one batch, got none");
        assert!(
            batches.len() <= 2,
            "expected debounced to <=2 batches, got {}",
            batches.len()
        );
    }
}
