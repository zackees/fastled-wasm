//! Cross-platform path utilities.
//!
//! `NormalizedPath` is a thin wrapper around `PathBuf` whose constructor
//! enforces a normalized representation:
//! - Lexically resolves `.` and `..` (no filesystem access, no symlink walk).
//! - Strips the Windows long-path prefix (`\\?\`) that
//!   [`std::fs::canonicalize`] adds. Many external tools (meson, Python's
//!   `open()`, emcc) cannot handle that prefix.
//! - Builds a case-insensitive comparison key on Windows / macOS for stable
//!   equality and hashing across spellings.
//!
//! This is a focused port of `zccache_core::path::NormalizedPath` (see
//! https://github.com/zackees/zccache/blob/main/crates/zccache/src/core/path.rs).
//! It exists to give `fastled-cli` a single boundary type for any path that
//! crosses into an external process, so regressions like #114 cannot recur.

use std::cmp::Ordering;
use std::ffi::OsStr;
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::path::{Component, Path, PathBuf};

/// A normalized, platform-aware path.
#[derive(Debug, Clone)]
pub struct NormalizedPath {
    path: PathBuf,
    case_key: Option<String>,
}

impl NormalizedPath {
    /// Build a normalized path from anything convertible to `Path`.
    pub fn new(path: impl AsRef<Path>) -> Self {
        let normalized = normalize(path.as_ref());
        let case_key = if cfg!(windows) || cfg!(target_os = "macos") {
            Some(normalize_for_key(&normalized))
        } else {
            None
        };
        Self {
            path: normalized,
            case_key,
        }
    }

    /// Borrow as a `Path`.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.path
    }

    /// Consume into an owned `PathBuf`. Use this only when handing to an API
    /// that requires `PathBuf` — at that point you've crossed the boundary
    /// where the wrapper's invariants stop being checked.
    #[must_use]
    pub fn into_path_buf(self) -> PathBuf {
        self.path
    }

    /// Case-insensitive comparison key. Populated on Windows / macOS.
    #[must_use]
    pub fn case_key(&self) -> Option<&str> {
        self.case_key.as_deref()
    }

    /// Build a new normalized path by joining a segment.
    #[must_use]
    pub fn join(&self, segment: impl AsRef<Path>) -> Self {
        Self::new(self.path.join(segment))
    }

    /// Display the normalized path as a string. Round-trips through
    /// `String::from(p.display().to_string())`; not lossless for non-UTF8
    /// paths but matches what we already do everywhere else.
    #[must_use]
    pub fn display_string(&self) -> String {
        self.path.display().to_string()
    }
}

impl PartialEq for NormalizedPath {
    fn eq(&self, other: &Self) -> bool {
        normalize_for_key(&self.path) == normalize_for_key(&other.path)
    }
}

impl Eq for NormalizedPath {}

impl Hash for NormalizedPath {
    fn hash<H: Hasher>(&self, state: &mut H) {
        normalize_for_key(&self.path).hash(state);
    }
}

impl PartialOrd for NormalizedPath {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for NormalizedPath {
    fn cmp(&self, other: &Self) -> Ordering {
        normalize_for_key(&self.path).cmp(&normalize_for_key(&other.path))
    }
}

impl AsRef<Path> for NormalizedPath {
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}

impl AsRef<OsStr> for NormalizedPath {
    fn as_ref(&self) -> &OsStr {
        self.as_path().as_os_str()
    }
}

impl Deref for NormalizedPath {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        self.as_path()
    }
}

impl From<PathBuf> for NormalizedPath {
    fn from(path: PathBuf) -> Self {
        Self::new(path)
    }
}

impl From<&Path> for NormalizedPath {
    fn from(path: &Path) -> Self {
        Self::new(path)
    }
}

impl From<&str> for NormalizedPath {
    fn from(path: &str) -> Self {
        Self::new(path)
    }
}

impl From<String> for NormalizedPath {
    fn from(path: String) -> Self {
        Self::new(path)
    }
}

/// Lexically normalize a path: resolve `.` / `..` and strip the Windows
/// long-path prefix. Does not touch the filesystem.
#[must_use]
pub fn normalize(path: &Path) -> PathBuf {
    let stripped = strip_windows_long_path_prefix(path);
    let mut components = Vec::new();
    for component in stripped.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if let Some(Component::Normal(_)) = components.last() {
                    components.pop();
                } else {
                    components.push(component);
                }
            }
            other => components.push(other),
        }
    }
    components.iter().collect()
}

/// Strip a leading `\\?\` (and `\\?\UNC\` → `\\`) from a Windows long-path
/// form. No-op on non-Windows and on paths that don't carry the prefix.
fn strip_windows_long_path_prefix(path: &Path) -> PathBuf {
    if !cfg!(windows) {
        return path.to_path_buf();
    }
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        return PathBuf::from(rest.to_string());
    }
    path.to_path_buf()
}

/// Stable string key for a path: separator normalization, long-path prefix
/// removal, case folding on case-insensitive filesystems.
#[must_use]
pub fn normalize_for_key(path: &Path) -> String {
    let normalized = normalize(path);

    #[cfg(windows)]
    {
        let mut s = normalized.to_string_lossy().replace('\\', "/");
        if let Some(stripped) = s.strip_prefix("//?/") {
            s = stripped.to_string();
        }
        s.make_ascii_lowercase();
        s
    }
    #[cfg(target_os = "macos")]
    {
        normalized.to_string_lossy().to_lowercase()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        normalized.to_string_lossy().into_owned()
    }
}

/// Canonicalize via the filesystem, then normalize. Falls back to the input
/// path if canonicalization fails (e.g. the path doesn't exist yet).
#[must_use]
pub fn canonicalize_normalized(path: &Path) -> NormalizedPath {
    match std::fs::canonicalize(path) {
        Ok(p) => NormalizedPath::new(p),
        Err(_) => NormalizedPath::new(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_curdir_and_parentdir() {
        assert_eq!(normalize(Path::new("a/./b")), PathBuf::from("a/b"));
        assert_eq!(normalize(Path::new("a/b/../c")), PathBuf::from("a/c"));
    }

    #[cfg(windows)]
    #[test]
    fn normalize_strips_windows_long_path_prefix() {
        let normalized = normalize(Path::new(r"\\?\C:\Users\me\repo"));
        assert_eq!(normalized, PathBuf::from(r"C:\Users\me\repo"));
    }

    #[cfg(windows)]
    #[test]
    fn normalize_handles_unc_shares() {
        let normalized = normalize(Path::new(r"\\?\UNC\server\share\dir"));
        assert_eq!(normalized, PathBuf::from(r"\\server\share\dir"));
    }

    #[cfg(windows)]
    #[test]
    fn normalize_for_key_folds_case_and_separators() {
        let a = normalize_for_key(Path::new(r"\\?\C:\Work\src\..\src\main.cpp"));
        let b = normalize_for_key(Path::new("c:/work/src/main.cpp"));
        assert_eq!(a, b);
    }

    #[test]
    fn normalized_path_equality_uses_normalized_form() {
        let a = NormalizedPath::new("a/./b/c");
        let b = NormalizedPath::new("a/b/c");
        assert_eq!(a, b);
    }

    #[test]
    fn normalized_path_join_returns_normalized() {
        let base = NormalizedPath::new("/tmp/work");
        let joined = base.join("./sub/../target");
        assert_eq!(joined.as_path(), Path::new("/tmp/work/target"));
    }
}
