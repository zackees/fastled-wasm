use pyo3::prelude::*;
use std::path::PathBuf;
use std::process::Command;

// ---------------------------------------------------------------------------
// Internal: Tauri viewer discovery (mirrors viewer.rs in fastled-cli)
// ---------------------------------------------------------------------------

#[cfg(windows)]
const VIEWER_EXE: &str = "fastled-viewer.exe";
#[cfg(not(windows))]
const VIEWER_EXE: &str = "fastled-viewer";

fn find_tauri_viewer_path() -> Option<PathBuf> {
    // 1. Sibling of the running shared library / executable.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join(VIEWER_EXE);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    // 2. Walk up from the current executable looking for a Cargo workspace root,
    //    then check target/debug and target/release.
    if let Some(root) = find_workspace_root_py() {
        for profile in &["debug", "release"] {
            let candidate = root.join("target").join(profile).join(VIEWER_EXE);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    // 3. PATH lookup.
    let on_path = Command::new(VIEWER_EXE)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if on_path {
        return Some(PathBuf::from(VIEWER_EXE));
    }

    None
}

fn find_workspace_root_py() -> Option<PathBuf> {
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

// ---------------------------------------------------------------------------
// PyO3 functions
// ---------------------------------------------------------------------------

/// Return the native module version.
#[pyfunction]
fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Return whether the native Rust file watcher is available in this build.
///
/// Python callers can use this to decide whether to prefer the Rust watcher
/// over the Python watchdog implementation.
///
/// ```python
/// from fastled._native import watch_available
/// if watch_available():
///     print("native watcher ready")
/// ```
#[pyfunction]
fn watch_available() -> bool {
    true
}

/// Return whether the native Rust archive download and extraction utilities
/// are available in this build.
///
/// ```python
/// from fastled._native import archive_available
/// if archive_available():
///     print("native archive support ready")
/// ```
#[pyfunction]
fn archive_available() -> bool {
    true
}

/// Return whether the native Rust project initialisation and sketch detection
/// utilities are available in this build.
///
/// ```python
/// from fastled._native import project_available
/// if project_available():
///     print("native project init ready")
/// ```
#[pyfunction]
fn project_available() -> bool {
    true
}

/// Return whether the native Rust build orchestration module is available in
/// this build.
///
/// ```python
/// from fastled._native import build_available
/// if build_available():
///     print("native build orchestration ready")
/// ```
#[pyfunction]
fn build_available() -> bool {
    true
}

/// Return whether the native Tauri viewer binary (`fastled-viewer`) can be
/// found alongside the CLI, in the Cargo target directory, or on PATH.
///
/// Python callers can use this to decide whether to prefer the native viewer
/// over the legacy Flask + Playwright browser experience.
///
/// ```python
/// from fastled._native import viewer_available
/// if viewer_available():
///     print("native viewer ready")
/// ```
#[pyfunction]
fn viewer_available() -> bool {
    find_tauri_viewer_path().is_some()
}

/// FastLED native extension module.
#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_function(wrap_pyfunction!(watch_available, m)?)?;
    m.add_function(wrap_pyfunction!(archive_available, m)?)?;
    m.add_function(wrap_pyfunction!(project_available, m)?)?;
    m.add_function(wrap_pyfunction!(build_available, m)?)?;
    m.add_function(wrap_pyfunction!(viewer_available, m)?)?;
    Ok(())
}
