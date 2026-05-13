//! Build orchestration for WASM compilation.
//!
//! The Rust CLI now drives the Python build service in-process through PyO3
//! instead of shelling out through `python -m fastled.app`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use pyo3::exceptions::PyKeyboardInterrupt;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};

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
    /// Return the Python enum member name.
    pub fn py_member_name(&self) -> &'static str {
        match self {
            BuildMode::Quick => "QUICK",
            BuildMode::Debug => "DEBUG",
            BuildMode::Release => "RELEASE",
        }
    }

    /// Return a human-readable label.
    pub fn label(&self) -> &'static str {
        self.py_member_name()
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

/// Mirrors the useful fields returned from `build_types.BuildResult`.
#[derive(Debug)]
pub struct BuildResult {
    /// Whether the compilation succeeded.
    pub success: bool,
    /// Directory that received the compiled artefacts.
    pub output_dir: PathBuf,
    /// Wall-clock seconds spent inside the in-process Python build call.
    pub duration_secs: f64,
    /// Python-side sketch compilation time.
    pub sketch_time_secs: f64,
    /// Strategy selected by the build service (`cold` or `incremental`).
    pub strategy: String,
    /// Final summary line from the Python build result.
    pub output: String,
}

// ---------------------------------------------------------------------------
// Python bridge helpers
// ---------------------------------------------------------------------------

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")))
}

fn workspace_python_source() -> Option<PathBuf> {
    let src_dir = workspace_root().join("src");
    src_dir.is_dir().then_some(src_dir)
}

fn normalize_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Canonicalize a sketch/`fastled_path` so the toolchain key registered with
/// `NativeBuildService.register_toolchain` matches the key passed into
/// `NativeBuildService.build` — mirrors `_toolchain_key` in
/// `src/fastled/build_service.py`.
fn toolchain_key(fastled_path: Option<&Path>) -> Option<String> {
    fastled_path.map(|p| normalize_path(p).to_string_lossy().into_owned())
}

fn ensure_workspace_src_on_sys_path(py: Python<'_>) -> PyResult<()> {
    let Some(src_dir) = workspace_python_source() else {
        return Ok(());
    };
    let src_text = src_dir.to_string_lossy().into_owned();
    let sys = PyModule::import(py, "sys")?;
    let sys_path = sys.getattr("path")?;
    let contains: bool = sys_path
        .call_method1("__contains__", (src_text.as_str(),))?
        .extract()?;
    if !contains {
        sys_path.call_method1("insert", (0, src_text.as_str()))?;
    }
    Ok(())
}

fn run_build_embedded(request: &BuildRequest) -> Result<BuildResult> {
    let start = Instant::now();
    Python::with_gil(|py| -> Result<BuildResult> {
        ensure_workspace_src_on_sys_path(py)
            .context("failed to add workspace Python sources to sys.path")?;

        let native_mod =
            PyModule::import(py, "fastled._native").context("import fastled._native")?;
        let types_mod = PyModule::import(py, "fastled.types").context("import fastled.types")?;

        let build_mode_obj = types_mod
            .getattr("BuildMode")
            .context("BuildMode enum missing")?
            .getattr(request.build_mode.py_member_name())
            .with_context(|| {
                format!("BuildMode.{} missing", request.build_mode.py_member_name())
            })?;

        let service = native_mod
            .getattr("NativeBuildService")
            .context("NativeBuildService class missing")?
            .call0()
            .context("construct NativeBuildService")?;

        // Register the Emscripten toolchain inline (still Python-owned).
        let toolchain_mod = PyModule::import(py, "fastled.toolchain.emscripten")
            .context("import fastled.toolchain.emscripten")?;
        let toolchain_kwargs = PyDict::new(py);
        match &request.fastled_path {
            Some(path) => toolchain_kwargs
                .set_item("fastled_path", path.to_string_lossy().as_ref())
                .context("set toolchain fastled_path")?,
            None => toolchain_kwargs
                .set_item("fastled_path", py.None())
                .context("set toolchain fastled_path none")?,
        }
        let toolchain = toolchain_mod
            .getattr("EmscriptenToolchain")
            .context("EmscriptenToolchain class missing")?
            .call((), Some(&toolchain_kwargs))
            .context("construct EmscriptenToolchain")?;

        let key = toolchain_key(request.fastled_path.as_deref());
        service
            .call_method1("register_toolchain", (toolchain, key.clone()))
            .context("register_toolchain")?;

        let sketch_dir_str = request.sketch_dir.to_string_lossy().into_owned();
        let build_mode_str = request.build_mode.py_member_name();

        let build_call = service.call_method1(
            "build",
            (
                sketch_dir_str.as_str(),
                build_mode_str,
                build_mode_obj,
                request.profile,
                key.clone(),
                request.force_clean,
            ),
        );

        let result = match build_call {
            Ok(r) => r,
            Err(err) => {
                if err.is_instance_of::<PyKeyboardInterrupt>(py) {
                    // Mirror the Python wrapper: route through
                    // fastled.interrupts.handle_keyboard_interrupt so a
                    // worker-thread Ctrl+C wakes the main thread.
                    if let Ok(interrupts) = PyModule::import(py, "fastled.interrupts") {
                        if let Ok(handler) = interrupts.getattr("handle_keyboard_interrupt") {
                            let _ = handler.call1((err.clone_ref(py),));
                        }
                    }
                }
                return Err(err).context("NativeBuildService.build failed");
            }
        };

        let result_dict = result
            .downcast_into::<PyDict>()
            .map_err(|e| anyhow::anyhow!("NativeBuildService.build returned non-dict: {e}"))?;

        let success: bool = result_dict
            .get_item("success")
            .context("missing build result success")?
            .ok_or_else(|| anyhow::anyhow!("missing build result success"))?
            .extract()
            .context("extract build success")?;
        let output: String = result_dict
            .get_item("stdout")
            .context("missing build result stdout")?
            .ok_or_else(|| anyhow::anyhow!("missing build result stdout"))?
            .extract()
            .context("extract build stdout")?;
        let strategy: String = result_dict
            .get_item("strategy")
            .context("missing build strategy")?
            .ok_or_else(|| anyhow::anyhow!("missing build strategy"))?
            .extract()
            .context("extract build strategy")?;
        let sketch_time_secs: f64 = result_dict
            .get_item("sketch_time")
            .context("missing sketch_time")?
            .ok_or_else(|| anyhow::anyhow!("missing sketch_time"))?
            .extract()
            .context("extract sketch_time")?;
        let output_dir_str: String = result_dict
            .get_item("output_dir")
            .context("missing output_dir")?
            .ok_or_else(|| anyhow::anyhow!("missing output_dir"))?
            .extract()
            .context("extract output_dir")?;
        let output_dir = PathBuf::from(output_dir_str);

        Ok(BuildResult {
            success,
            output_dir,
            duration_secs: start.elapsed().as_secs_f64(),
            sketch_time_secs,
            strategy,
            output,
        })
    })
}

fn run_build_subprocess(request: &BuildRequest, reason: &str) -> Result<BuildResult> {
    use std::process::Command;

    let python = if Command::new("python")
        .args(["--version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        "python"
    } else {
        "python3"
    };

    let output_dir = request.output_dir();
    let mut cmd = Command::new(python);
    cmd.args(["-m", "fastled.app", "--just-compile"]);
    cmd.arg(&request.sketch_dir);

    match request.build_mode {
        BuildMode::Quick => {}
        BuildMode::Debug => {
            cmd.arg("--debug");
        }
        BuildMode::Release => {
            cmd.arg("--release");
        }
    }

    if request.profile {
        cmd.arg("--profile");
    }
    if let Some(fp) = &request.fastled_path {
        cmd.arg("--fastled-path");
        cmd.arg(fp);
    }
    if request.force_clean {
        cmd.arg("--purge");
    }

    eprintln!("fastled: falling back to subprocess build path: {reason}");
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
        sketch_time_secs: duration_secs,
        strategy: "unknown".to_string(),
        output: combined,
    })
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Run a WASM build through the Python build service without the legacy
/// `fastled.app` CLI trampoline.
pub fn run_build(request: &BuildRequest) -> Result<BuildResult> {
    match run_build_embedded(request) {
        Ok(result) => Ok(result),
        Err(err) => run_build_subprocess(request, &format!("{err:#}")),
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // BuildMode
    // ------------------------------------------------------------------

    #[test]
    fn test_build_mode_member_names() {
        assert_eq!(BuildMode::Quick.py_member_name(), "QUICK");
        assert_eq!(BuildMode::Debug.py_member_name(), "DEBUG");
        assert_eq!(BuildMode::Release.py_member_name(), "RELEASE");
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

    #[test]
    fn test_workspace_python_source_points_at_repo_src() {
        let src_dir = workspace_python_source().expect("workspace src dir");
        assert!(src_dir.ends_with("src"));
        assert!(src_dir.is_dir());
    }

    #[test]
    fn test_toolchain_key_is_none_when_unset() {
        assert_eq!(toolchain_key(None), None);
    }

    #[test]
    fn test_toolchain_key_returns_some_when_set() {
        // The canonicalize call may fail for nonexistent paths and fall back
        // to the input path, which is fine for this test.
        let p = PathBuf::from("/tmp/fastled-test-keypath-does-not-exist");
        let key = toolchain_key(Some(&p));
        assert!(key.is_some());
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
            sketch_time_secs: 1.75,
            strategy: "cold".to_string(),
            output: "Build OK".to_string(),
        };

        assert!(result.success);
        assert_eq!(result.duration_secs, 2.5);
        assert_eq!(result.sketch_time_secs, 1.75);
        assert_eq!(result.strategy, "cold");
        assert_eq!(result.output, "Build OK");
    }
}
