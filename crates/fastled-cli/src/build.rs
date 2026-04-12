//! Build orchestration for WASM compilation.
//!
//! Ports the high-level build request / result abstraction from
//! `build_types.py` and `build_service.py` to Rust.
//!
//! The actual WASM compilation remains in Python.  [`run_build`] delegates
//! to `python -m fastled.app --just-compile` as a subprocess, keeping the
//! Rust layer as a thin orchestration shell.
//!
//! This module is included as a library component; the CLI main loop will
//! integrate it in a later phase for captured-output builds.

// The CLI currently uses `run_python_compile()` in main.rs (inherited stdio).
// This module provides a captured-output alternative for future use.
#![allow(dead_code)]

use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

use anyhow::{Context, Result};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Mirrors `BuildMode` from `types.py`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildMode {
    Quick,
    Debug,
    Release,
}

impl BuildMode {
    /// Return the CLI flag string that the Python CLI expects.
    pub fn as_flag(&self) -> &'static str {
        match self {
            BuildMode::Quick => "--quick",
            BuildMode::Debug => "--debug",
            BuildMode::Release => "--release",
        }
    }

    /// Return a human-readable label.
    pub fn label(&self) -> &'static str {
        match self {
            BuildMode::Quick => "QUICK",
            BuildMode::Debug => "DEBUG",
            BuildMode::Release => "RELEASE",
        }
    }
}

impl std::fmt::Display for BuildMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// Mirrors `BuildRequest` from `build_types.py`.
#[derive(Debug, Clone)]
pub struct BuildRequest {
    /// Directory that contains the FastLED sketch.
    pub sketch_dir: PathBuf,
    /// Which optimisation profile to use.
    pub build_mode: BuildMode,
    /// Enable C++ build-system profiling output.
    pub profile: bool,
    /// Optional path to a local FastLED source tree.
    pub fastled_path: Option<PathBuf>,
    /// Discard cached artefacts and start from scratch.
    pub force_clean: bool,
}

impl BuildRequest {
    /// Derive the conventional output directory for this request.
    pub fn output_dir(&self) -> PathBuf {
        self.sketch_dir.join("fastled_js")
    }
}

/// Mirrors `BuildResult` from `build_types.py`.
#[derive(Debug)]
pub struct BuildResult {
    /// Whether the compilation succeeded.
    pub success: bool,
    /// Directory that received the compiled artefacts.
    pub output_dir: PathBuf,
    /// Wall-clock seconds spent in the Python subprocess.
    pub duration_secs: f64,
    /// Combined stdout + stderr from the Python subprocess.
    pub output: String,
}

// ---------------------------------------------------------------------------
// Subprocess helper
// ---------------------------------------------------------------------------

/// Locate a Python executable.
///
/// Priority order (mirrors `find_python` in `main.rs`):
/// 1. `VIRTUAL_ENV/Scripts/python.exe` (Windows) or `VIRTUAL_ENV/bin/python`
/// 2. `python` on PATH
/// 3. `python3` on PATH
fn find_python() -> String {
    if let Ok(venv) = std::env::var("VIRTUAL_ENV") {
        #[cfg(windows)]
        let candidate = format!("{}/Scripts/python.exe", venv);
        #[cfg(not(windows))]
        let candidate = format!("{}/bin/python", venv);

        if std::path::Path::new(&candidate).exists() {
            return candidate;
        }
    }

    if Command::new("python")
        .args(["--version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        return "python".to_string();
    }

    "python3".to_string()
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Run a WASM build by calling the Python CLI as a subprocess.
///
/// Delegates to `python -m fastled.app --just-compile <sketch_dir> [flags]`.
/// The actual Emscripten / WASM compilation stays entirely in Python; this
/// function only constructs the command, measures elapsed time, and wraps the
/// result.
pub fn run_build(request: &BuildRequest) -> Result<BuildResult> {
    let python = find_python();
    let output_dir = request.output_dir();

    let mut cmd = Command::new(&python);
    cmd.args(["-m", "fastled.app", "--just-compile"]);
    cmd.arg(&request.sketch_dir);
    cmd.arg(request.build_mode.as_flag());

    if request.profile {
        cmd.arg("--profile");
    }
    if let Some(fp) = &request.fastled_path {
        cmd.arg("--fastled-path");
        cmd.arg(fp);
    }

    let start = Instant::now();
    let output = cmd
        .output()
        .with_context(|| format!("failed to launch `{python} -m fastled.app`"))?;
    let duration_secs = start.elapsed().as_secs_f64();

    let combined = {
        let mut s = String::new();
        s.push_str(&String::from_utf8_lossy(&output.stdout));
        if !output.stderr.is_empty() {
            s.push('\n');
            s.push_str(&String::from_utf8_lossy(&output.stderr));
        }
        s
    };

    Ok(BuildResult {
        success: output.status.success(),
        output_dir,
        duration_secs,
        output: combined,
    })
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // ------------------------------------------------------------------
    // BuildMode
    // ------------------------------------------------------------------

    #[test]
    fn test_build_mode_flags() {
        assert_eq!(BuildMode::Quick.as_flag(), "--quick");
        assert_eq!(BuildMode::Debug.as_flag(), "--debug");
        assert_eq!(BuildMode::Release.as_flag(), "--release");
    }

    #[test]
    fn test_build_mode_labels() {
        assert_eq!(BuildMode::Quick.label(), "QUICK");
        assert_eq!(BuildMode::Debug.label(), "DEBUG");
        assert_eq!(BuildMode::Release.label(), "RELEASE");
    }

    #[test]
    fn test_build_mode_display() {
        assert_eq!(format!("{}", BuildMode::Quick), "QUICK");
        assert_eq!(format!("{}", BuildMode::Debug), "DEBUG");
        assert_eq!(format!("{}", BuildMode::Release), "RELEASE");
    }

    #[test]
    fn test_build_mode_equality() {
        assert_eq!(BuildMode::Quick, BuildMode::Quick);
        assert_ne!(BuildMode::Quick, BuildMode::Debug);
        assert_ne!(BuildMode::Debug, BuildMode::Release);
    }

    // ------------------------------------------------------------------
    // BuildRequest
    // ------------------------------------------------------------------

    #[test]
    fn test_build_request_construction() {
        let sketch = PathBuf::from("/tmp/my_sketch");
        let req = BuildRequest {
            sketch_dir: sketch.clone(),
            build_mode: BuildMode::Quick,
            profile: false,
            fastled_path: None,
            force_clean: false,
        };

        assert_eq!(req.sketch_dir, sketch);
        assert_eq!(req.build_mode, BuildMode::Quick);
        assert!(!req.profile);
        assert!(req.fastled_path.is_none());
        assert!(!req.force_clean);
    }

    #[test]
    fn test_build_request_output_dir() {
        let sketch = PathBuf::from("/tmp/my_sketch");
        let req = BuildRequest {
            sketch_dir: sketch.clone(),
            build_mode: BuildMode::Debug,
            profile: false,
            fastled_path: None,
            force_clean: false,
        };

        assert_eq!(req.output_dir(), sketch.join("fastled_js"));
    }

    #[test]
    fn test_build_request_with_fastled_path() {
        let sketch = PathBuf::from("/tmp/my_sketch");
        let fl_path = PathBuf::from("/opt/fastled");
        let req = BuildRequest {
            sketch_dir: sketch,
            build_mode: BuildMode::Release,
            profile: true,
            fastled_path: Some(fl_path.clone()),
            force_clean: true,
        };

        assert_eq!(req.fastled_path, Some(fl_path));
        assert!(req.profile);
        assert!(req.force_clean);
        assert_eq!(req.build_mode, BuildMode::Release);
    }

    // ------------------------------------------------------------------
    // BuildResult
    // ------------------------------------------------------------------

    #[test]
    fn test_build_result_fields() {
        let result = BuildResult {
            success: true,
            output_dir: PathBuf::from("/tmp/my_sketch/fastled_js"),
            duration_secs: 2.5,
            output: "Build OK".to_string(),
        };

        assert!(result.success);
        assert_eq!(result.duration_secs, 2.5);
        assert_eq!(result.output, "Build OK");
    }
}
