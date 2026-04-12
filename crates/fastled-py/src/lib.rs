use pyo3::prelude::*;

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

/// FastLED native extension module.
#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_function(wrap_pyfunction!(watch_available, m)?)?;
    m.add_function(wrap_pyfunction!(archive_available, m)?)?;
    m.add_function(wrap_pyfunction!(project_available, m)?)?;
    Ok(())
}
