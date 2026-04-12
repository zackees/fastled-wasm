//! Tauri viewer discovery and launch utilities.
//!
//! Provides [`find_tauri_viewer`] to locate the `fastled-viewer` binary and
//! [`launch_tauri_viewer`] to spawn it against a compiled output directory.

use std::path::PathBuf;
use std::process::{Child, Command};

use anyhow::{Context, Result};

// ---------------------------------------------------------------------------
// Binary name (platform-aware)
// ---------------------------------------------------------------------------

#[cfg(windows)]
const VIEWER_EXE: &str = "fastled-viewer.exe";
#[cfg(not(windows))]
const VIEWER_EXE: &str = "fastled-viewer";

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

/// Search for the `fastled-viewer` (Tauri) binary.
///
/// Search order:
/// 1. Same directory as the currently running executable.
/// 2. `target/debug/` relative to the workspace root (detected via the
///    executable path heuristic or `CARGO_MANIFEST_DIR`).
/// 3. `target/release/` via the same heuristic.
/// 4. `PATH` — `which`-style lookup via [`Command::new`].
///
/// Returns `None` if the binary cannot be found.
pub fn find_tauri_viewer() -> Option<PathBuf> {
    // 1. Sibling of the running executable.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join(VIEWER_EXE);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    // 2. Walk up from the current executable to find a Cargo workspace root
    //    (directory that contains a `Cargo.toml` with `[workspace]`), then
    //    check `target/debug` and `target/release`.
    if let Some(workspace_root) = find_workspace_root() {
        for profile in &["debug", "release"] {
            let candidate = workspace_root.join("target").join(profile).join(VIEWER_EXE);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    // 3. Fall back to PATH lookup.
    if is_on_path(VIEWER_EXE) {
        // Return just the bare name so the OS resolves it through PATH.
        return Some(PathBuf::from(VIEWER_EXE));
    }

    None
}

/// Return `true` when the Tauri viewer binary can be found.
#[inline]
pub fn viewer_available() -> bool {
    find_tauri_viewer().is_some()
}

// ---------------------------------------------------------------------------
// Launch
// ---------------------------------------------------------------------------

/// Spawn the Tauri viewer, pointing it at `frontend_dir`.
///
/// The viewer is launched as a **detached child process** — the caller is not
/// expected to `wait()` on it; FastLED will keep running (serving files, etc.)
/// alongside the viewer window.
///
/// Returns the [`Child`] handle so the caller can optionally monitor the
/// process lifetime.
pub fn launch_tauri_viewer(frontend_dir: &std::path::Path) -> Result<Child> {
    let binary = find_tauri_viewer()
        .context("fastled-viewer binary not found; cannot launch Tauri viewer")?;

    let child = Command::new(&binary)
        .arg("--frontend-dir")
        .arg(frontend_dir)
        .spawn()
        .with_context(|| format!("failed to spawn fastled-viewer from '{}'", binary.display()))?;

    Ok(child)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Walk up the filesystem from the current executable until a directory
/// containing a `Cargo.toml` file is found.  This is a heuristic to locate
/// the workspace root during development; it will gracefully return `None` in
/// production installs where there is no `Cargo.toml`.
fn find_workspace_root() -> Option<PathBuf> {
    let start = std::env::current_exe().ok()?;
    let mut dir = start.parent()?.to_path_buf();

    for _ in 0..10 {
        if dir.join("Cargo.toml").is_file() {
            return Some(dir.clone());
        }
        match dir.parent() {
            Some(p) => dir = p.to_path_buf(),
            None => break,
        }
    }
    None
}

/// Check whether `name` resolves on PATH by attempting a no-op invocation.
fn is_on_path(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_viewer_available_does_not_panic() {
        // We don't assert a specific value — the binary may or may not be
        // present in CI.  We only verify no panic occurs.
        let _ = viewer_available();
    }

    #[test]
    fn test_find_tauri_viewer_returns_option() {
        // Same as above: just confirm the function runs without panicking.
        let result = find_tauri_viewer();
        // If found, the path must have a file name component.
        if let Some(p) = result {
            assert!(p.file_name().is_some(), "expected a non-empty path");
        }
    }

    #[test]
    fn test_find_workspace_root_does_not_panic() {
        let _ = find_workspace_root();
    }
}
