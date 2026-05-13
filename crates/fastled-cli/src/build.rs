//! Build orchestration for WASM compilation.
//!
//! The Rust CLI now drives the Python build service in-process through PyO3
//! instead of shelling out through `python -m fastled.app`.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
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

fn py_path(py: Python<'_>, path: &Path) -> PyResult<Py<PyAny>> {
    let pathlib = PyModule::import(py, "pathlib")?;
    let cls = pathlib.getattr("Path")?;
    Ok(cls
        .call1((path.to_string_lossy().as_ref(),))?
        .into_any()
        .unbind())
}

fn py_fspath(value: &Bound<'_, PyAny>) -> PyResult<PathBuf> {
    let py = value.py();
    let os = PyModule::import(py, "os")?;
    let fspath = os.getattr("fspath")?;
    let path_value = fspath.call1((value,))?;
    let path_str: String = path_value.extract()?;
    Ok(PathBuf::from(path_str))
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

        let build_service_mod = PyModule::import(py, "fastled.build_service")
            .context("import fastled.build_service")?;
        let build_types_mod =
            PyModule::import(py, "fastled.build_types").context("import fastled.build_types")?;
        let types_mod = PyModule::import(py, "fastled.types").context("import fastled.types")?;

        let build_mode = types_mod
            .getattr("BuildMode")
            .context("BuildMode enum missing")?
            .getattr(request.build_mode.py_member_name())
            .with_context(|| {
                format!("BuildMode.{} missing", request.build_mode.py_member_name())
            })?;

        let request_kwargs = PyDict::new(py);
        request_kwargs
            .set_item("sketch_dir", py_path(py, &request.sketch_dir)?)
            .context("set sketch_dir")?;
        request_kwargs
            .set_item("build_mode", &build_mode)
            .context("set build_mode")?;
        request_kwargs
            .set_item("profile", request.profile)
            .context("set profile")?;
        match &request.fastled_path {
            Some(path) => request_kwargs
                .set_item("fastled_path", py_path(py, path)?)
                .context("set fastled_path")?,
            None => request_kwargs
                .set_item("fastled_path", py.None())
                .context("set fastled_path none")?,
        }
        request_kwargs
            .set_item("force_clean", request.force_clean)
            .context("set force_clean")?;

        let build_request = build_types_mod
            .getattr("BuildRequest")
            .context("BuildRequest class missing")?
            .call((), Some(&request_kwargs))
            .context("construct BuildRequest")?;

        let service = build_service_mod
            .getattr("BuildService")
            .context("BuildService class missing")?
            .call0()
            .context("construct BuildService")?;

        let result = service
            .call_method1("build", (build_request,))
            .context("BuildService.build failed")?;

        let success: bool = result
            .getattr("success")
            .context("missing build result success")?
            .extract()
            .context("extract build success")?;
        let output: String = result
            .getattr("stdout")
            .context("missing build result stdout")?
            .extract()
            .context("extract build stdout")?;
        let strategy: String = result
            .getattr("strategy")
            .context("missing build strategy")?
            .extract()
            .context("extract build strategy")?;
        let sketch_time_secs: f64 = result
            .getattr("sketch_time")
            .context("missing sketch_time")?
            .extract()
            .context("extract sketch_time")?;
        let output_dir = py_fspath(&result.getattr("output_dir").context("missing output_dir")?)
            .context("extract output_dir")?;

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
