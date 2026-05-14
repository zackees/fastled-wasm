use fastled_cli::frontend;
use fastled_cli::install;
use fastled_cli::project;
use fastled_cli::viewer;
use fastled_cli::wasm_build;
use fastled_cli::{PromptChoice, SketchSelection};
use pyo3::exceptions::{PyKeyError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyModule};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;
use zip::write::SimpleFileOptions;

const DEFAULT_EXAMPLE: &str = "wasm";

// ---------------------------------------------------------------------------
// Internal: Native BuildService
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct BuildState {
    build_mode: String,
    profile: bool,
    fastled_path: Option<String>,
}

fn normalize_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn resolve_state_key(path: &Path) -> PathBuf {
    normalize_path(path)
}

fn state_file(output_dir: &Path) -> PathBuf {
    output_dir.join(".fastled_build_state.json")
}

fn write_state(output_dir: &Path, state: &BuildState) {
    let payload = json!({
        "build_mode": state.build_mode,
        "profile": state.profile,
        "fastled_path": state.fastled_path,
    });
    if let Ok(serialized) = serde_json::to_string(&payload) {
        let _ = fs::write(state_file(output_dir), serialized);
    }
}

fn read_state(output_dir: &Path) -> Option<BuildState> {
    let payload = fs::read_to_string(state_file(output_dir)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&payload).ok()?;
    let build_mode = value.get("build_mode")?.as_str()?.to_string();
    let profile = value.get("profile")?.as_bool()?;
    let fastled_path = match value.get("fastled_path") {
        Some(serde_json::Value::String(path)) => Some(path.clone()),
        Some(serde_json::Value::Null) | None => None,
        _ => return None,
    };
    Some(BuildState {
        build_mode,
        profile,
        fastled_path,
    })
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out)?;
        } else if path.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

fn zip_output(output_dir: &Path) -> io::Result<Vec<u8>> {
    let mut files = Vec::new();
    collect_files(output_dir, &mut files)?;
    files.sort();

    let cursor = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(cursor);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    for file_path in files {
        let relative = file_path
            .strip_prefix(output_dir)
            .unwrap_or(file_path.as_path())
            .to_string_lossy()
            .replace('\\', "/");
        zip.start_file(relative, options)?;
        zip.write_all(&fs::read(&file_path)?)?;
    }

    Ok(zip.finish()?.into_inner())
}

#[derive(Default)]
struct NativeBuildArtifacts {
    js: Option<PathBuf>,
    wasm: Option<PathBuf>,
    frontend_assets: Option<PathBuf>,
}

fn existing_path(path: PathBuf) -> Option<PathBuf> {
    path.exists().then_some(path)
}

fn discover_artifacts(output_dir: &Path) -> NativeBuildArtifacts {
    let frontend_assets = existing_path(output_dir.join("assets"))
        .or_else(|| output_dir.exists().then_some(output_dir.to_path_buf()));

    NativeBuildArtifacts {
        js: existing_path(output_dir.join("fastled.js")),
        wasm: existing_path(output_dir.join("fastled.wasm")),
        frontend_assets,
    }
}

fn python_path(py: Python<'_>, path: &Path) -> PyResult<Py<PyAny>> {
    let pathlib = PyModule::import(py, "pathlib")?;
    let cls = pathlib.getattr("Path")?;
    Ok(cls
        .call1((path.to_string_lossy().as_ref(),))?
        .into_any()
        .unbind())
}

fn python_optional_path(py: Python<'_>, path: Option<&Path>) -> PyResult<Py<PyAny>> {
    match path {
        Some(path) => python_path(py, path),
        None => Ok(py.None()),
    }
}

fn path_from_py(value: &Bound<'_, PyAny>) -> PyResult<PathBuf> {
    let os = PyModule::import(value.py(), "os")?;
    let path_obj = os.getattr("fspath")?.call1((value,))?;
    Ok(PathBuf::from(path_obj.extract::<String>()?))
}

fn optional_path_from_py(value: Option<&Bound<'_, PyAny>>) -> PyResult<Option<PathBuf>> {
    match value {
        Some(value) if !value.is_none() => Ok(Some(path_from_py(value)?)),
        _ => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// Internal: string_diff port
// ---------------------------------------------------------------------------

fn token_sort_ratio(a: &str, b: &str) -> f64 {
    let mut a_tokens: Vec<&str> = a.split_whitespace().collect();
    let mut b_tokens: Vec<&str> = b.split_whitespace().collect();
    a_tokens.sort_unstable();
    b_tokens.sort_unstable();
    let a_sorted = a_tokens.join(" ");
    let b_sorted = b_tokens.join(" ");
    ratcliff_obershelp_ratio(&a_sorted, &b_sorted) * 100.0
}

fn ratcliff_obershelp_ratio(a: &str, b: &str) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    let matches = matching_chars(a_bytes, b_bytes) as f64;
    (2.0 * matches) / ((a_bytes.len() + b_bytes.len()) as f64)
}

fn matching_chars(a: &[u8], b: &[u8]) -> usize {
    let Some((a_pos, b_pos, len)) = longest_common_substring(a, b) else {
        return 0;
    };
    if len == 0 {
        return 0;
    }

    len + matching_chars(&a[..a_pos], &b[..b_pos])
        + matching_chars(&a[a_pos + len..], &b[b_pos + len..])
}

fn longest_common_substring(a: &[u8], b: &[u8]) -> Option<(usize, usize, usize)> {
    if a.is_empty() || b.is_empty() {
        return None;
    }

    let mut best = (0usize, 0usize, 0usize);
    let mut dp = vec![0usize; b.len() + 1];

    for (i, a_byte) in a.iter().enumerate() {
        for j in (0..b.len()).rev() {
            if *a_byte == b[j] {
                dp[j + 1] = dp[j] + 1;
                if dp[j + 1] > best.2 {
                    best = (i + 1 - dp[j + 1], j + 1 - dp[j + 1], dp[j + 1]);
                }
            } else {
                dp[j + 1] = 0;
            }
        }
    }

    if best.2 == 0 {
        None
    } else {
        Some(best)
    }
}

fn filter_out_obvious_bad_choices(input_str: &str, string_list: &[String]) -> Vec<String> {
    if input_str.trim().is_empty() {
        return string_list.to_vec();
    }

    let input_chars: std::collections::HashSet<char> = input_str.to_lowercase().chars().collect();
    let mut filtered = Vec::new();

    for candidate in string_list {
        let candidate_chars: std::collections::HashSet<char> =
            candidate.to_lowercase().chars().collect();
        let common_chars = input_chars.intersection(&candidate_chars).count();
        if (common_chars as f64) >= (input_chars.len() as f64 / 2.0) {
            filtered.push(candidate.clone());
        }
    }

    filtered
}

fn is_in_order_match_impl(input_str: &str, other: &str) -> bool {
    let input_chars: Vec<char> = input_str
        .chars()
        .filter(|c| *c != ' ')
        .map(|c| c.to_ascii_lowercase())
        .collect();
    let other_chars: Vec<char> = other.chars().map(|c| c.to_ascii_lowercase()).collect();

    let mut input_index = 0usize;
    let mut other_index = 0usize;

    while input_index < input_chars.len() && other_index < other_chars.len() {
        if input_chars[input_index] == other_chars[other_index] {
            input_index += 1;
        }
        other_index += 1;
    }

    input_index == input_chars.len()
}

fn normalize_for_matching(value: &str, ignore_case: bool) -> String {
    if ignore_case {
        value.to_lowercase()
    } else {
        value.to_owned()
    }
}

fn string_diff_impl(
    input_string: &str,
    string_list: &[String],
    ignore_case: bool,
) -> Vec<(f64, String)> {
    if input_string.trim().is_empty() {
        return string_list
            .iter()
            .enumerate()
            .map(|(i, s)| (i as f64, s.clone()))
            .collect();
    }

    if string_list.is_empty() {
        return Vec::new();
    }

    let original_string_list = string_list.to_vec();
    let normalized_input = normalize_for_matching(input_string, ignore_case);
    let normalized_strings: Vec<String> = string_list
        .iter()
        .map(|s| normalize_for_matching(s, ignore_case))
        .collect();
    let normalized_to_original: HashMap<String, String> = normalized_strings
        .iter()
        .cloned()
        .zip(string_list.iter().cloned())
        .collect();

    let exact_matches: Vec<String> = normalized_strings
        .iter()
        .filter(|s| **s == normalized_input)
        .cloned()
        .collect();
    let substring_matches: Vec<String> = normalized_strings
        .iter()
        .filter(|s| s.contains(&normalized_input))
        .cloned()
        .collect();

    if exact_matches.len() == 1 && substring_matches.len() > 1 {
        let exact_match = &exact_matches[0];
        let other_substring_matches: Vec<&String> = substring_matches
            .iter()
            .filter(|s| *s != exact_match)
            .collect();
        let mut should_prioritize_exact = true;
        let original_exact_match = normalized_to_original
            .get(exact_match)
            .cloned()
            .unwrap_or_else(|| exact_match.clone());

        for other_match in other_substring_matches {
            let original_other_match = normalized_to_original
                .get(other_match)
                .cloned()
                .unwrap_or_else(|| other_match.clone());

            if !original_other_match
                .to_lowercase()
                .starts_with(&original_exact_match.to_lowercase())
            {
                should_prioritize_exact = false;
                break;
            }

            let remainder = &original_other_match[original_exact_match.len()..];
            if remainder
                .chars()
                .next()
                .map(|c| c.is_uppercase())
                .unwrap_or(false)
            {
                if original_exact_match.len() <= 4 && remainder.len() >= 6 {
                    continue;
                }
                should_prioritize_exact = false;
                break;
            }

            should_prioritize_exact = false;
            break;
        }

        if should_prioritize_exact {
            return exact_matches
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    (
                        i as f64,
                        normalized_to_original
                            .get(s)
                            .cloned()
                            .unwrap_or_else(|| s.clone()),
                    )
                })
                .collect();
        }

        let should_apply_char_filter = original_exact_match.len() >= 5
            && original_exact_match.chars().any(|c| c.is_ascii_digit())
            && original_exact_match
                .chars()
                .last()
                .map(|c| c.is_ascii_digit() || c.is_ascii_lowercase())
                .unwrap_or(false);

        if should_apply_char_filter {
            let max_extra_chars = if original_exact_match.len() <= 6 {
                std::cmp::min(10usize, original_exact_match.len() * 2)
            } else {
                12usize
            };

            let filtered_matches: Vec<String> = substring_matches
                .iter()
                .filter_map(|s| {
                    let original = normalized_to_original
                        .get(s)
                        .cloned()
                        .unwrap_or_else(|| s.clone());
                    if s == exact_match {
                        Some(s.clone())
                    } else {
                        let extra_chars = original.len().saturating_sub(original_exact_match.len());
                        (extra_chars <= max_extra_chars).then_some(s.clone())
                    }
                })
                .collect();

            return filtered_matches
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    (
                        i as f64,
                        normalized_to_original
                            .get(s)
                            .cloned()
                            .unwrap_or_else(|| s.clone()),
                    )
                })
                .collect();
        }

        return substring_matches
            .iter()
            .enumerate()
            .map(|(i, s)| {
                (
                    i as f64,
                    normalized_to_original
                        .get(s)
                        .cloned()
                        .unwrap_or_else(|| s.clone()),
                )
            })
            .collect();
    }

    if !exact_matches.is_empty() && substring_matches.len() == 1 {
        return exact_matches
            .iter()
            .enumerate()
            .map(|(i, s)| {
                (
                    i as f64,
                    normalized_to_original
                        .get(s)
                        .cloned()
                        .unwrap_or_else(|| s.clone()),
                )
            })
            .collect();
    }

    let mut working_list = normalized_strings.clone();
    if normalized_input.trim().chars().count() >= 3 {
        let filtered = filter_out_obvious_bad_choices(&normalized_input, &working_list);
        if !filtered.is_empty() {
            working_list = filtered;
        }
    }

    if !substring_matches.is_empty() {
        return substring_matches
            .iter()
            .enumerate()
            .map(|(i, s)| {
                (
                    i as f64,
                    normalized_to_original
                        .get(s)
                        .cloned()
                        .unwrap_or_else(|| s.clone()),
                )
            })
            .collect();
    }

    let in_order_matches: Vec<String> = working_list
        .iter()
        .filter(|s| is_in_order_match_impl(&normalized_input, s))
        .cloned()
        .collect();
    if !in_order_matches.is_empty() {
        working_list = in_order_matches;
    }

    let mut distances: Vec<f64> = working_list
        .iter()
        .map(|s| 1.0 / (token_sort_ratio(&normalized_input, s) + 1.0))
        .collect();

    if distances.is_empty() {
        working_list = original_string_list
            .iter()
            .map(|s| normalize_for_matching(s, ignore_case))
            .collect();
        distances = working_list
            .iter()
            .map(|s| 1.0 / (token_sort_ratio(&normalized_input, s) + 1.0))
            .collect();
    }

    let min_distance = distances
        .iter()
        .fold(f64::INFINITY, |best, value| best.min(*value));

    distances
        .iter()
        .enumerate()
        .filter(|(_, distance)| (**distance - min_distance).abs() < 1e-12)
        .map(|(i, _)| {
            let normalized = &working_list[i];
            (
                i as f64,
                normalized_to_original
                    .get(normalized)
                    .cloned()
                    .unwrap_or_else(|| normalized.clone()),
            )
        })
        .collect()
}

#[pyclass(name = "BuildMode", frozen)]
#[derive(Clone, Debug, Eq, PartialEq)]
struct PyBuildMode {
    name: String,
    value: String,
}

impl PyBuildMode {
    fn from_label(label: &str) -> PyResult<Self> {
        let normalized = label.trim().to_ascii_uppercase();
        match normalized.as_str() {
            "DEBUG" => Ok(Self {
                name: "DEBUG".to_string(),
                value: "DEBUG".to_string(),
            }),
            "QUICK" => Ok(Self {
                name: "QUICK".to_string(),
                value: "QUICK".to_string(),
            }),
            "RELEASE" => Ok(Self {
                name: "RELEASE".to_string(),
                value: "RELEASE".to_string(),
            }),
            other => Err(PyValueError::new_err(format!(
                "BUILD_MODE must be one of ['DEBUG', 'QUICK', 'RELEASE'], got {other}"
            ))),
        }
    }

    fn from_py(value: &Bound<'_, PyAny>) -> PyResult<Self> {
        if let Ok(mode) = value.extract::<PyRef<'_, PyBuildMode>>() {
            return Ok(mode.clone());
        }
        if let Ok(attr) = value.getattr("value") {
            if let Ok(text) = attr.extract::<String>() {
                return Self::from_label(&text);
            }
        }
        if let Ok(text) = value.extract::<String>() {
            return Self::from_label(&text);
        }
        Err(PyValueError::new_err(
            "expected BuildMode or build mode string",
        ))
    }

    fn as_wasm_mode(&self) -> PyResult<wasm_build::BuildMode> {
        match self.value.as_str() {
            "QUICK" => Ok(wasm_build::BuildMode::Quick),
            "DEBUG" => Ok(wasm_build::BuildMode::Debug),
            "RELEASE" => Ok(wasm_build::BuildMode::Release),
            other => Err(PyValueError::new_err(format!(
                "unsupported build mode: {other}"
            ))),
        }
    }
}

#[pymethods]
#[allow(non_snake_case)]
impl PyBuildMode {
    #[new]
    fn new(mode: &str) -> PyResult<Self> {
        Self::from_label(mode)
    }

    #[classattr]
    fn DEBUG() -> Self {
        Self::from_label("DEBUG").expect("valid build mode")
    }

    #[classattr]
    fn QUICK() -> Self {
        Self::from_label("QUICK").expect("valid build mode")
    }

    #[classattr]
    fn RELEASE() -> Self {
        Self::from_label("RELEASE").expect("valid build mode")
    }

    #[staticmethod]
    fn from_string(mode: &str) -> PyResult<Self> {
        Self::from_label(mode)
    }

    #[staticmethod]
    fn from_args(args: &Bound<'_, PyAny>) -> PyResult<Self> {
        let debug = args
            .getattr("debug")
            .ok()
            .and_then(|value| value.extract::<bool>().ok())
            .unwrap_or(false);
        let release = args
            .getattr("release")
            .ok()
            .and_then(|value| value.extract::<bool>().ok())
            .unwrap_or(false);
        if debug {
            Self::from_label("DEBUG")
        } else if release {
            Self::from_label("RELEASE")
        } else {
            Self::from_label("QUICK")
        }
    }

    #[getter]
    fn name(&self) -> &str {
        &self.name
    }

    #[getter]
    fn value(&self) -> &str {
        &self.value
    }

    fn __str__(&self) -> &str {
        &self.value
    }

    fn __repr__(&self) -> String {
        format!("BuildMode.{}", self.name)
    }
}

#[pyclass(name = "BuildRequest")]
#[derive(Clone)]
struct PyBuildRequest {
    sketch_dir: PathBuf,
    build_mode: PyBuildMode,
    profile: bool,
    fastled_path: Option<PathBuf>,
    force_clean: bool,
}

#[pymethods]
impl PyBuildRequest {
    #[new]
    #[pyo3(signature = (sketch_dir, build_mode, profile=false, fastled_path=None, force_clean=false))]
    fn new(
        sketch_dir: &Bound<'_, PyAny>,
        build_mode: &Bound<'_, PyAny>,
        profile: bool,
        fastled_path: Option<&Bound<'_, PyAny>>,
        force_clean: bool,
    ) -> PyResult<Self> {
        Ok(Self {
            sketch_dir: path_from_py(sketch_dir)?,
            build_mode: PyBuildMode::from_py(build_mode)?,
            profile,
            fastled_path: optional_path_from_py(fastled_path)?,
            force_clean,
        })
    }

    #[getter]
    fn sketch_dir(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        python_path(py, &self.sketch_dir)
    }

    #[getter]
    fn build_mode(&self, py: Python<'_>) -> PyResult<Py<PyBuildMode>> {
        Py::new(py, self.build_mode.clone())
    }

    #[getter]
    fn profile(&self) -> bool {
        self.profile
    }

    #[getter]
    fn fastled_path(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        python_optional_path(py, self.fastled_path.as_deref())
    }

    #[getter]
    fn force_clean(&self) -> bool {
        self.force_clean
    }

    #[getter]
    fn output_dir(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        python_path(py, &self.sketch_dir.join("fastled_js"))
    }
}

#[pyclass(name = "BuildArtifacts")]
#[derive(Clone, Default)]
struct PyBuildArtifacts {
    js: Option<PathBuf>,
    wasm: Option<PathBuf>,
    frontend_assets: Option<PathBuf>,
}

impl PyBuildArtifacts {
    fn from_native(value: NativeBuildArtifacts) -> Self {
        Self {
            js: value.js,
            wasm: value.wasm,
            frontend_assets: value.frontend_assets,
        }
    }

    fn path_by_name(&self, name: &str) -> Option<&Path> {
        match name {
            "js" => self.js.as_deref(),
            "wasm" => self.wasm.as_deref(),
            "frontend_assets" => self.frontend_assets.as_deref(),
            _ => None,
        }
    }
}

#[pymethods]
impl PyBuildArtifacts {
    #[new]
    #[pyo3(signature = (js=None, wasm=None, frontend_assets=None))]
    fn new(
        js: Option<&Bound<'_, PyAny>>,
        wasm: Option<&Bound<'_, PyAny>>,
        frontend_assets: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        Ok(Self {
            js: optional_path_from_py(js)?,
            wasm: optional_path_from_py(wasm)?,
            frontend_assets: optional_path_from_py(frontend_assets)?,
        })
    }

    #[getter]
    fn js(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        python_optional_path(py, self.js.as_deref())
    }

    #[getter]
    fn wasm(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        python_optional_path(py, self.wasm.as_deref())
    }

    #[getter]
    fn frontend_assets(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        python_optional_path(py, self.frontend_assets.as_deref())
    }

    fn as_dict(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let dict = PyDict::new(py);
        for name in ["js", "wasm", "frontend_assets"] {
            if let Some(path) = self.path_by_name(name) {
                dict.set_item(name, python_path(py, path)?)?;
            }
        }
        Ok(dict.unbind())
    }

    fn __getitem__(&self, py: Python<'_>, name: &str) -> PyResult<Py<PyAny>> {
        self.path_by_name(name)
            .map(|path| python_path(py, path))
            .unwrap_or_else(|| Err(PyKeyError::new_err(name.to_string())))
    }

    #[pyo3(signature = (name, default=None))]
    fn get(&self, py: Python<'_>, name: &str, default: Option<Py<PyAny>>) -> PyResult<Py<PyAny>> {
        match self.path_by_name(name) {
            Some(path) => python_path(py, path),
            None => Ok(default.unwrap_or_else(|| py.None())),
        }
    }

    fn items(&self, py: Python<'_>) -> PyResult<Vec<(String, Py<PyAny>)>> {
        let mut out = Vec::new();
        for name in ["js", "wasm", "frontend_assets"] {
            if let Some(path) = self.path_by_name(name) {
                out.push((name.to_string(), python_path(py, path)?));
            }
        }
        Ok(out)
    }
}

#[pyclass(name = "CompileResult")]
#[derive(Clone)]
struct PyCompileResult {
    success: bool,
    stdout: String,
    hash_value: Option<String>,
    zip_bytes: Vec<u8>,
    zip_time: f64,
    libfastled_time: f64,
    sketch_time: f64,
    response_processing_time: f64,
}

#[pymethods]
impl PyCompileResult {
    #[new]
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (success, stdout, hash_value=None, zip_bytes=None, zip_time=0.0, libfastled_time=0.0, sketch_time=0.0, response_processing_time=0.0))]
    fn new(
        success: bool,
        stdout: String,
        hash_value: Option<String>,
        zip_bytes: Option<&Bound<'_, PyAny>>,
        zip_time: f64,
        libfastled_time: f64,
        sketch_time: f64,
        response_processing_time: f64,
    ) -> PyResult<Self> {
        let zip_bytes = match zip_bytes {
            Some(value) if !value.is_none() => value.extract::<Vec<u8>>()?,
            _ => Vec::new(),
        };
        Ok(Self {
            success,
            stdout,
            hash_value,
            zip_bytes,
            zip_time,
            libfastled_time,
            sketch_time,
            response_processing_time,
        })
    }

    #[getter]
    fn success(&self) -> bool {
        self.success
    }

    #[getter]
    fn stdout(&self) -> &str {
        &self.stdout
    }

    #[getter]
    fn hash_value(&self) -> Option<String> {
        self.hash_value.clone()
    }

    #[getter]
    fn zip_bytes(&self, py: Python<'_>) -> Py<PyAny> {
        PyBytes::new(py, &self.zip_bytes).into_any().unbind()
    }

    #[getter]
    fn zip_time(&self) -> f64 {
        self.zip_time
    }

    #[getter]
    fn libfastled_time(&self) -> f64 {
        self.libfastled_time
    }

    #[getter]
    fn sketch_time(&self) -> f64 {
        self.sketch_time
    }

    #[getter]
    fn response_processing_time(&self) -> f64 {
        self.response_processing_time
    }

    fn __bool__(&self) -> bool {
        self.success
    }

    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let dict = PyDict::new(py);
        dict.set_item("success", self.success)?;
        dict.set_item("stdout", &self.stdout)?;
        dict.set_item("hash_value", self.hash_value.clone())?;
        dict.set_item("zip_bytes", PyBytes::new(py, &self.zip_bytes))?;
        dict.set_item("zip_time", self.zip_time)?;
        dict.set_item("libfastled_time", self.libfastled_time)?;
        dict.set_item("sketch_time", self.sketch_time)?;
        dict.set_item("response_processing_time", self.response_processing_time)?;
        Ok(dict.unbind())
    }
}

#[pyclass(name = "BuildResult")]
#[derive(Clone)]
struct PyBuildResult {
    compile_result: PyCompileResult,
    strategy: String,
    output_dir: PathBuf,
    artifacts: PyBuildArtifacts,
}

#[pymethods]
impl PyBuildResult {
    #[new]
    #[pyo3(signature = (compile_result, strategy, output_dir, artifacts))]
    fn new(
        compile_result: PyRef<'_, PyCompileResult>,
        strategy: String,
        output_dir: &Bound<'_, PyAny>,
        artifacts: PyRef<'_, PyBuildArtifacts>,
    ) -> PyResult<Self> {
        Ok(Self {
            compile_result: compile_result.clone(),
            strategy,
            output_dir: path_from_py(output_dir)?,
            artifacts: artifacts.clone(),
        })
    }

    #[getter]
    fn compile_result(&self, py: Python<'_>) -> PyResult<Py<PyCompileResult>> {
        Py::new(py, self.compile_result.clone())
    }

    #[getter]
    fn strategy(&self) -> &str {
        &self.strategy
    }

    #[getter]
    fn output_dir(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        python_path(py, &self.output_dir)
    }

    #[getter]
    fn artifacts(&self, py: Python<'_>) -> PyResult<Py<PyBuildArtifacts>> {
        Py::new(py, self.artifacts.clone())
    }

    #[getter]
    fn success(&self) -> bool {
        self.compile_result.success
    }

    #[getter]
    fn stdout(&self) -> &str {
        &self.compile_result.stdout
    }

    #[getter]
    fn sketch_time(&self) -> f64 {
        self.compile_result.sketch_time
    }

    #[getter]
    fn zip_bytes(&self, py: Python<'_>) -> Py<PyAny> {
        PyBytes::new(py, &self.compile_result.zip_bytes)
            .into_any()
            .unbind()
    }
}

#[pyclass(name = "BuildService")]
struct PyBuildService {
    states: HashMap<PathBuf, BuildState>,
}

#[pymethods]
impl PyBuildService {
    #[new]
    fn new() -> Self {
        Self {
            states: HashMap::new(),
        }
    }

    #[pyo3(signature = (fastled_path=None, toolchain=None))]
    fn register_toolchain(
        &mut self,
        fastled_path: Option<&Bound<'_, PyAny>>,
        toolchain: Option<Py<PyAny>>,
    ) {
        let _ = (fastled_path, toolchain);
    }

    fn detect_strategy(&mut self, request: PyRef<'_, PyBuildRequest>) -> String {
        self.detect_strategy_inner(
            &request.sketch_dir,
            &request.build_mode.value,
            request.profile,
            request
                .fastled_path
                .as_deref()
                .map(|path| normalize_path(path).to_string_lossy().into_owned()),
            request.force_clean,
        )
    }

    fn build(
        &mut self,
        py: Python<'_>,
        request: PyRef<'_, PyBuildRequest>,
    ) -> PyResult<Py<PyBuildResult>> {
        let output_dir = request.sketch_dir.join("fastled_js");
        let fastled_path = request
            .fastled_path
            .as_deref()
            .map(|path| normalize_path(path).to_string_lossy().into_owned());
        let strategy = self.detect_strategy_inner(
            &request.sketch_dir,
            &request.build_mode.value,
            request.profile,
            fastled_path.clone(),
            request.force_clean,
        );

        if request.force_clean && output_dir.exists() {
            let _ = fs::remove_dir_all(&output_dir);
        }

        fs::create_dir_all(&output_dir).map_err(|err| PyRuntimeError::new_err(err.to_string()))?;

        let native_request = wasm_build::BuildRequest {
            sketch_dir: request.sketch_dir.clone(),
            build_mode: request.build_mode.as_wasm_mode()?,
            profile: request.profile,
            fastled_path: request.fastled_path.clone(),
            force_clean: request.force_clean,
        };

        match wasm_build::run_build(&native_request) {
            Ok(build_result) if build_result.success => {
                let zip_start = Instant::now();
                let zip_bytes = zip_output(&build_result.output_dir)
                    .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
                let zip_time = zip_start.elapsed().as_secs_f64();

                let state = BuildState {
                    build_mode: request.build_mode.value.clone(),
                    profile: request.profile,
                    fastled_path: fastled_path.clone(),
                };
                self.states
                    .insert(resolve_state_key(&request.sketch_dir), state.clone());
                write_state(&output_dir, &state);

                Self::build_result(
                    py,
                    true,
                    build_result.output,
                    zip_bytes,
                    zip_time,
                    build_result.sketch_time_secs,
                    build_result.strategy,
                    &build_result.output_dir,
                )
            }
            Ok(build_result) => Self::build_result(
                py,
                false,
                build_result.output,
                Vec::new(),
                0.0,
                build_result.sketch_time_secs,
                build_result.strategy,
                &build_result.output_dir,
            ),
            Err(err) => Self::build_result(
                py,
                false,
                format!("Native Rust WASM build failed: {err:#}"),
                Vec::new(),
                0.0,
                0.0,
                strategy,
                &output_dir,
            ),
        }
    }

    fn purge(&mut self, sketch_dir: &Bound<'_, PyAny>) -> PyResult<()> {
        let sketch_dir = path_from_py(sketch_dir)?;
        self.states.remove(&resolve_state_key(&sketch_dir));
        let output_dir = sketch_dir.join("fastled_js");
        if output_dir.exists() {
            let _ = fs::remove_dir_all(output_dir);
        }
        Ok(())
    }
}

impl PyBuildService {
    fn detect_strategy_inner(
        &mut self,
        sketch_dir: &Path,
        build_mode: &str,
        profile: bool,
        fastled_path: Option<String>,
        force_clean: bool,
    ) -> String {
        if force_clean {
            return "cold".to_string();
        }

        let output_dir = sketch_dir.join("fastled_js");
        if !output_dir.exists() {
            return "cold".to_string();
        }

        let required_artifacts = [
            output_dir.join("fastled.js"),
            output_dir.join("fastled.wasm"),
        ];
        if required_artifacts.iter().any(|path| !path.exists()) {
            return "cold".to_string();
        }

        let sketch_key = resolve_state_key(sketch_dir);
        let previous = if let Some(state) = self.states.get(&sketch_key).cloned() {
            Some(state)
        } else if let Some(state) = read_state(&output_dir) {
            self.states.insert(sketch_key.clone(), state.clone());
            Some(state)
        } else {
            None
        };

        let Some(previous) = previous else {
            return "cold".to_string();
        };

        if previous.fastled_path != fastled_path {
            return "cold".to_string();
        }
        if previous.build_mode != build_mode {
            return "cold".to_string();
        }
        if previous.profile != profile {
            return "cold".to_string();
        }

        "incremental".to_string()
    }

    #[allow(clippy::too_many_arguments)]
    fn build_result(
        py: Python<'_>,
        success: bool,
        stdout: String,
        zip_bytes: Vec<u8>,
        zip_time: f64,
        sketch_time: f64,
        strategy: String,
        output_dir: &Path,
    ) -> PyResult<Py<PyBuildResult>> {
        let compile_result = PyCompileResult {
            success,
            stdout,
            hash_value: None,
            zip_bytes,
            zip_time,
            libfastled_time: 0.0,
            sketch_time,
            response_processing_time: 0.0,
        };
        let artifacts = PyBuildArtifacts::from_native(discover_artifacts(output_dir));
        Py::new(
            py,
            PyBuildResult {
                compile_result,
                strategy,
                output_dir: output_dir.to_path_buf(),
                artifacts,
            },
        )
    }
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
#[pyfunction]
fn watch_available() -> bool {
    true
}

/// Return whether the native Rust archive download and extraction utilities
/// are available in this build.
#[pyfunction]
fn archive_available() -> bool {
    true
}

/// Return whether the native Rust project initialisation and sketch detection
/// utilities are available in this build.
#[pyfunction]
fn project_available() -> bool {
    true
}

#[pyfunction(signature = (ref_name=None))]
fn get_examples(ref_name: Option<&str>) -> PyResult<Vec<String>> {
    let cwd = std::env::current_dir()?;
    let repo_root = match project::find_fastled_repo_upwards(&cwd, 10) {
        Some(local_repo) => local_repo,
        None => install::ensure_fastled_repo(ref_name)
            .map_err(|err| PyRuntimeError::new_err(format!("{err:#}")))?,
    };
    Ok(project::collect_examples(&repo_root.join("examples")))
}

#[pyfunction(signature = (example=None, outputdir=None, ref_name=None))]
fn project_init(
    py: Python<'_>,
    example: Option<&str>,
    outputdir: Option<&Bound<'_, PyAny>>,
    ref_name: Option<&str>,
) -> PyResult<Py<PyAny>> {
    let output_dir = optional_path_from_py(outputdir)?.unwrap_or_else(|| PathBuf::from("fastled"));
    fs::create_dir_all(&output_dir).map_err(|err| PyRuntimeError::new_err(err.to_string()))?;

    let cwd = std::env::current_dir()?;
    let local_repo = project::find_fastled_repo_upwards(&cwd, 10);
    let repo_root = match local_repo {
        Some(path) => path,
        None => install::ensure_fastled_repo(ref_name)
            .map_err(|err| PyRuntimeError::new_err(format!("{err:#}")))?,
    };

    let selected = match example {
        Some(value) if value != "PROMPT" => value,
        _ => DEFAULT_EXAMPLE,
    };
    let resolved_ref = ref_name.map(|_| project::cached_repo_ref_name(&repo_root));
    let out =
        project::init_example_from_repo(&repo_root, selected, &output_dir, resolved_ref.as_deref())
            .map_err(|err| PyRuntimeError::new_err(format!("{err:#}")))?;
    python_path(py, &out)
}

#[pyfunction(signature = (directory=None))]
fn find_sketch_directories(directory: Option<&str>) -> PyResult<Vec<String>> {
    let root = directory
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir()?);
    Ok(project::find_sketches(&root)
        .into_iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect())
}

#[pyfunction(signature = (partial_name, search_dir=None))]
fn find_sketch_by_partial_name(partial_name: &str, search_dir: Option<&str>) -> PyResult<String> {
    let root = search_dir
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir()?);
    project::find_sketch_by_partial_name(partial_name, &root)
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|err| PyValueError::new_err(err.to_string()))
}

fn selection_payload(selection: SketchSelection) -> (String, Option<String>, Vec<String>) {
    match selection {
        SketchSelection::Selected(value) => ("selected".to_string(), Some(value), Vec::new()),
        SketchSelection::Prompt(options) => ("prompt".to_string(), None, options),
        SketchSelection::None => ("none".to_string(), None, Vec::new()),
    }
}

fn prompt_choice_payload(choice: PromptChoice) -> (String, Option<String>, Vec<String>) {
    match choice {
        PromptChoice::Selected(value) => ("selected".to_string(), Some(value), Vec::new()),
        PromptChoice::Narrowed(options) => ("narrowed".to_string(), None, options),
        PromptChoice::Retry => ("retry".to_string(), None, Vec::new()),
    }
}

#[pyfunction]
fn prepare_sketch_selection(
    sketch_directories: Vec<String>,
    cwd_is_fastled: bool,
    is_followup: bool,
) -> (String, Option<String>, Vec<String>) {
    let paths = sketch_directories.into_iter().map(PathBuf::from).collect();
    selection_payload(fastled_cli::prepare_sketch_selection(
        paths,
        cwd_is_fastled,
        is_followup,
    ))
}

#[pyfunction(signature = (input, options, default_index=0))]
fn resolve_prompt_choice(
    input: &str,
    options: Vec<String>,
    default_index: usize,
) -> PyResult<(String, Option<String>, Vec<String>)> {
    if options.is_empty() {
        return Err(PyValueError::new_err("options must not be empty"));
    }
    if default_index >= options.len() {
        return Err(PyValueError::new_err("default_index out of range"));
    }
    Ok(prompt_choice_payload(fastled_cli::resolve_prompt_choice(
        input,
        &options,
        default_index,
    )))
}

#[pyfunction(signature = (path=None))]
fn looks_like_fastled_repo(path: Option<&str>) -> PyResult<bool> {
    let directory = path.map(PathBuf::from).unwrap_or(std::env::current_dir()?);
    Ok(project::is_fastled_repo(&directory))
}

#[pyfunction(signature = (path=None, quick=false))]
fn looks_like_sketch_directory(path: Option<&str>, quick: bool) -> PyResult<bool> {
    let Some(directory) = path
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
    else {
        return Ok(false);
    };
    Ok(project::looks_like_sketch_directory(&directory, quick))
}

#[pyfunction(signature = (start=None, max_depth=10))]
fn find_fastled_repo_upwards(start: Option<&str>, max_depth: usize) -> PyResult<Option<String>> {
    let directory = start.map(PathBuf::from).unwrap_or(std::env::current_dir()?);
    Ok(project::find_fastled_repo_upwards(&directory, max_depth)
        .map(|path| path.to_string_lossy().into_owned()))
}

#[pyfunction]
fn collect_examples(examples_dir: &str) -> Vec<String> {
    project::collect_examples(&PathBuf::from(examples_dir))
}

#[pyfunction]
fn find_example_in_repo(repo_root: &str, example: &str) -> Option<String> {
    project::find_example_in_repo(&PathBuf::from(repo_root), example)
        .map(|path| path.to_string_lossy().into_owned())
}

#[pyfunction(signature = (example_name, dest, branch=None))]
fn init_example(example_name: &str, dest: &str, branch: Option<&str>) -> PyResult<String> {
    project::init_example(example_name, &PathBuf::from(dest), branch)
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|err| PyValueError::new_err(err.to_string()))
}

#[pyfunction(signature = (ref_name=None))]
fn ensure_fastled_repo(ref_name: Option<&str>) -> PyResult<String> {
    install::ensure_fastled_repo(ref_name)
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|err| PyValueError::new_err(err.to_string()))
}

#[pyfunction]
fn read_fastled_json_ref(directory: &str) -> Option<String> {
    project::read_fastled_json_ref(&PathBuf::from(directory))
}

#[pyfunction(signature = (repo_root, example_name, output_dir, ref_name=None))]
fn init_example_from_repo(
    repo_root: &str,
    example_name: &str,
    output_dir: &str,
    ref_name: Option<&str>,
) -> PyResult<String> {
    project::init_example_from_repo(
        &PathBuf::from(repo_root),
        example_name,
        &PathBuf::from(output_dir),
        ref_name,
    )
    .map(|path| path.to_string_lossy().into_owned())
    .map_err(|err| PyValueError::new_err(err.to_string()))
}

/// Return whether the native Rust build orchestration module is available in
/// this build.
#[pyfunction]
fn build_available() -> bool {
    true
}

/// Return whether the native Tauri viewer binary (`fastled-viewer`) can be
/// found alongside the CLI, in the Cargo target directory, or on PATH.
#[pyfunction]
fn viewer_available() -> bool {
    viewer::find_tauri_viewer().is_some()
}

/// Locate the `fastled-viewer` (Tauri) binary.  Returns `None` if not found.
///
/// Delegates to `fastled_cli::viewer::find_tauri_viewer` so the Python shim no
/// longer duplicates the discovery logic.
#[pyfunction]
fn find_fastled_viewer() -> Option<String> {
    viewer::find_tauri_viewer().map(|p| p.to_string_lossy().into_owned())
}

#[pyfunction]
fn is_in_order_match(input_str: &str, other: &str) -> bool {
    is_in_order_match_impl(input_str, other)
}

#[pyfunction(signature = (input_string, string_list, ignore_case=true))]
fn string_diff(
    input_string: &str,
    string_list: Vec<String>,
    ignore_case: bool,
) -> Vec<(f64, String)> {
    string_diff_impl(input_string, &string_list, ignore_case)
}

/// Build the FastLED frontend (if stale) and copy it into ``output_dir``.
///
/// Mirrors ``src/fastled/frontend_esbuild.py::copy_frontend_to_output`` so the
/// Python shim can delegate without re-implementing the bundling logic.
#[pyfunction]
#[pyo3(signature = (output_dir, source_dir=None))]
fn copy_frontend_to_output(
    output_dir: &Bound<'_, PyAny>,
    source_dir: Option<&Bound<'_, PyAny>>,
) -> PyResult<()> {
    let out = path_from_py(output_dir)?;
    let src = optional_path_from_py(source_dir)?;
    frontend::copy_frontend_to_output(&out, src.as_deref())
        .map_err(|e| PyRuntimeError::new_err(format!("{e:#}")))
}

/// FastLED native extension module.
#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyBuildMode>()?;
    m.add_class::<PyBuildRequest>()?;
    m.add_class::<PyBuildArtifacts>()?;
    m.add_class::<PyCompileResult>()?;
    m.add_class::<PyBuildResult>()?;
    m.add_class::<PyBuildService>()?;
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_function(wrap_pyfunction!(watch_available, m)?)?;
    m.add_function(wrap_pyfunction!(archive_available, m)?)?;
    m.add_function(wrap_pyfunction!(project_available, m)?)?;
    m.add_function(wrap_pyfunction!(get_examples, m)?)?;
    m.add_function(wrap_pyfunction!(project_init, m)?)?;
    m.add_function(wrap_pyfunction!(find_sketch_directories, m)?)?;
    m.add_function(wrap_pyfunction!(find_sketch_by_partial_name, m)?)?;
    m.add_function(wrap_pyfunction!(prepare_sketch_selection, m)?)?;
    m.add_function(wrap_pyfunction!(resolve_prompt_choice, m)?)?;
    m.add_function(wrap_pyfunction!(looks_like_fastled_repo, m)?)?;
    m.add_function(wrap_pyfunction!(looks_like_sketch_directory, m)?)?;
    m.add_function(wrap_pyfunction!(find_fastled_repo_upwards, m)?)?;
    m.add_function(wrap_pyfunction!(collect_examples, m)?)?;
    m.add_function(wrap_pyfunction!(find_example_in_repo, m)?)?;
    m.add_function(wrap_pyfunction!(init_example, m)?)?;
    m.add_function(wrap_pyfunction!(ensure_fastled_repo, m)?)?;
    m.add_function(wrap_pyfunction!(read_fastled_json_ref, m)?)?;
    m.add_function(wrap_pyfunction!(init_example_from_repo, m)?)?;
    m.add_function(wrap_pyfunction!(build_available, m)?)?;
    m.add_function(wrap_pyfunction!(viewer_available, m)?)?;
    m.add_function(wrap_pyfunction!(find_fastled_viewer, m)?)?;
    m.add_function(wrap_pyfunction!(is_in_order_match, m)?)?;
    m.add_function(wrap_pyfunction!(string_diff, m)?)?;
    m.add_function(wrap_pyfunction!(copy_frontend_to_output, m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use fastled_cli::wasm_build;
    use pyo3::prelude::*;

    use super::{
        is_in_order_match_impl, path_from_py, string_diff_impl, write_state, BuildState,
        PyBuildArtifacts, PyBuildMode, PyBuildRequest, PyBuildService, PyCompileResult,
    };

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time before unix epoch")
                .as_nanos();
            let path = std::env::temp_dir()
                .join(format!("fastled-py-{name}-{}-{nanos}", std::process::id()));
            fs::create_dir_all(&path).expect("create temp test directory");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn write_build_outputs(sketch_dir: &Path, state: BuildState) {
        let output_dir = sketch_dir.join("fastled_js");
        fs::create_dir_all(&output_dir).expect("create output directory");
        fs::write(output_dir.join("fastled.js"), "js").expect("write fastled.js");
        fs::write(output_dir.join("fastled.wasm"), "wasm").expect("write fastled.wasm");
        write_state(&output_dir, &state);
    }

    #[test]
    fn test_build_mode_normalizes_labels_and_maps_to_wasm_modes() {
        let quick = PyBuildMode::from_label(" quick ").expect("quick mode");
        assert_eq!(quick.name, "QUICK");
        assert_eq!(quick.value, "QUICK");
        assert_eq!(quick.__str__(), "QUICK");
        assert_eq!(quick.__repr__(), "BuildMode.QUICK");
        assert_eq!(
            quick.as_wasm_mode().expect("quick wasm mode"),
            wasm_build::BuildMode::Quick
        );

        let debug = PyBuildMode::from_label("debug").expect("debug mode");
        assert_eq!(
            debug.as_wasm_mode().expect("debug wasm mode"),
            wasm_build::BuildMode::Debug
        );

        let release = PyBuildMode::from_label("RELEASE").expect("release mode");
        assert_eq!(
            release.as_wasm_mode().expect("release wasm mode"),
            wasm_build::BuildMode::Release
        );
    }

    #[test]
    fn test_build_mode_rejects_unknown_label() {
        assert!(PyBuildMode::from_label("fast").is_err());
    }

    #[test]
    fn test_build_request_defaults_to_fastled_js_output_dir() {
        let sketch_dir = PathBuf::from("sketch");
        let request = PyBuildRequest {
            sketch_dir: sketch_dir.clone(),
            build_mode: PyBuildMode::from_label("quick").expect("quick mode"),
            profile: false,
            fastled_path: None,
            force_clean: false,
        };

        assert_eq!(request.sketch_dir, sketch_dir);
        assert_eq!(request.build_mode.value, "QUICK");
        assert!(!request.profile);
        assert!(request.fastled_path.is_none());
        assert!(!request.force_clean);

        Python::with_gil(|py| {
            let output_dir = request.output_dir(py).expect("output_dir");
            assert_eq!(
                path_from_py(output_dir.bind(py)).expect("path from output_dir"),
                PathBuf::from("sketch").join("fastled_js")
            );
        });
    }

    #[test]
    fn test_build_artifacts_exposes_present_paths_only() {
        let js_path = PathBuf::from("out").join("fastled.js");
        let wasm_path = PathBuf::from("out").join("fastled.wasm");
        let artifacts = PyBuildArtifacts {
            js: Some(js_path.clone()),
            wasm: Some(wasm_path.clone()),
            frontend_assets: None,
        };

        assert_eq!(artifacts.path_by_name("js"), Some(js_path.as_path()));
        assert_eq!(artifacts.path_by_name("wasm"), Some(wasm_path.as_path()));
        assert_eq!(artifacts.path_by_name("frontend_assets"), None);
        assert_eq!(artifacts.path_by_name("missing"), None);

        Python::with_gil(|py| {
            let js = artifacts.__getitem__(py, "js").expect("js item");
            assert_eq!(
                path_from_py(js.bind(py)).expect("path from js item"),
                js_path
            );
            assert!(artifacts.__getitem__(py, "missing").is_err());

            let dict = artifacts.as_dict(py).expect("artifacts dict");
            let dict = dict.bind(py);
            assert_eq!(dict.len(), 2);
            let wasm = dict
                .get_item("wasm")
                .expect("wasm lookup")
                .expect("wasm present");
            assert_eq!(
                path_from_py(&wasm).expect("path from wasm dict item"),
                wasm_path
            );
            assert!(dict
                .get_item("frontend_assets")
                .expect("frontend lookup")
                .is_none());
        });
    }

    #[test]
    fn test_compile_result_stores_bool_bytes_and_timing_fields() {
        let result = PyCompileResult {
            success: true,
            stdout: "ok".to_string(),
            hash_value: Some("abc123".to_string()),
            zip_bytes: b"abc".to_vec(),
            zip_time: 0.5,
            libfastled_time: 0.75,
            sketch_time: 1.25,
            response_processing_time: 0.125,
        };

        assert!(result.__bool__());
        assert_eq!(result.success, true);
        assert_eq!(result.stdout, "ok");
        assert_eq!(result.hash_value, Some("abc123".to_string()));
        assert_eq!(result.zip_bytes, b"abc");
        assert_eq!(result.sketch_time, 1.25);

        Python::with_gil(|py| {
            let dict = result.to_dict(py).expect("compile result dict");
            let dict = dict.bind(py);
            assert!(dict
                .get_item("success")
                .expect("success lookup")
                .expect("success present")
                .extract::<bool>()
                .expect("extract success"));
            assert_eq!(
                dict.get_item("stdout")
                    .expect("stdout lookup")
                    .expect("stdout present")
                    .extract::<String>()
                    .expect("extract stdout"),
                "ok"
            );
            assert_eq!(
                dict.get_item("zip_bytes")
                    .expect("zip lookup")
                    .expect("zip present")
                    .extract::<Vec<u8>>()
                    .expect("extract zip bytes"),
                b"abc"
            );
            assert_eq!(
                dict.get_item("sketch_time")
                    .expect("sketch_time lookup")
                    .expect("sketch_time present")
                    .extract::<f64>()
                    .expect("extract sketch_time"),
                1.25
            );
        });
    }

    #[test]
    fn test_build_service_detect_strategy_cold_when_output_missing() {
        let temp = TestDir::new("missing-output");
        let mut service = PyBuildService::new();

        assert_eq!(
            service.detect_strategy_inner(temp.path(), "QUICK", false, None, false),
            "cold"
        );
    }

    #[test]
    fn test_build_service_detect_strategy_cold_when_force_clean() {
        let temp = TestDir::new("force-clean");
        write_build_outputs(
            temp.path(),
            BuildState {
                build_mode: "QUICK".to_string(),
                profile: false,
                fastled_path: None,
            },
        );
        let mut service = PyBuildService::new();

        assert_eq!(
            service.detect_strategy_inner(temp.path(), "QUICK", false, None, true),
            "cold"
        );
    }

    #[test]
    fn test_build_service_detect_strategy_cold_when_artifacts_missing() {
        let temp = TestDir::new("missing-artifacts");
        let output_dir = temp.path().join("fastled_js");
        fs::create_dir_all(&output_dir).expect("create output directory");
        fs::write(output_dir.join("fastled.js"), "js").expect("write partial artifact");
        write_state(
            &output_dir,
            &BuildState {
                build_mode: "QUICK".to_string(),
                profile: false,
                fastled_path: None,
            },
        );
        let mut service = PyBuildService::new();

        assert_eq!(
            service.detect_strategy_inner(temp.path(), "QUICK", false, None, false),
            "cold"
        );
    }

    #[test]
    fn test_build_service_detect_strategy_incremental_when_state_matches() {
        let temp = TestDir::new("incremental");
        write_build_outputs(
            temp.path(),
            BuildState {
                build_mode: "QUICK".to_string(),
                profile: false,
                fastled_path: None,
            },
        );
        let mut service = PyBuildService::new();

        assert_eq!(
            service.detect_strategy_inner(temp.path(), "QUICK", false, None, false),
            "incremental"
        );
    }

    #[test]
    fn test_build_service_detect_strategy_cold_when_state_changes() {
        let temp = TestDir::new("state-changes");
        write_build_outputs(
            temp.path(),
            BuildState {
                build_mode: "QUICK".to_string(),
                profile: false,
                fastled_path: None,
            },
        );
        let mut service = PyBuildService::new();

        assert_eq!(
            service.detect_strategy_inner(temp.path(), "RELEASE", false, None, false),
            "cold"
        );
        assert_eq!(
            service.detect_strategy_inner(temp.path(), "QUICK", true, None, false),
            "cold"
        );
        assert_eq!(
            service.detect_strategy_inner(
                temp.path(),
                "QUICK",
                false,
                Some("different-fastled-path".to_string()),
                false,
            ),
            "cold"
        );
    }

    #[test]
    fn test_native_is_in_order_match() {
        assert!(is_in_order_match_impl("wave 2d", "Wave2d"));
        assert!(!is_in_order_match_impl("wz", "Wave2d"));
    }

    #[test]
    fn test_native_string_diff_prefers_fxwave2d_for_fxwave() {
        let haystack = vec![
            "examples\\Wave2d".to_owned(),
            "examples\\FxWave2d".to_owned(),
            "examples\\Blink".to_owned(),
        ];
        let result = string_diff_impl("FxWave", &haystack, true);
        assert_eq!(result[0].1, "examples\\FxWave2d");
    }

    #[test]
    fn test_native_string_diff_prioritizes_exact_wasm_match() {
        let haystack = vec!["wasm".to_owned(), "WasmScreenCoords".to_owned()];
        let result = string_diff_impl("Wasm", &haystack, true);
        assert_eq!(result, vec![(0.0, "wasm".to_owned())]);
    }

    #[test]
    fn test_native_string_diff_returns_variants_for_fire2012() {
        let haystack = vec![
            "Fire2012".to_owned(),
            "Fire2012WithPalette".to_owned(),
            "FxFire2012".to_owned(),
            "VeryLongPrefixFire2012".to_owned(),
        ];
        let result = string_diff_impl("Fire2012", &haystack, true);
        let names: Vec<String> = result.into_iter().map(|(_, name)| name).collect();
        assert!(names.contains(&"Fire2012".to_owned()));
        assert!(names.contains(&"FxFire2012".to_owned()));
        assert!(!names.contains(&"VeryLongPrefixFire2012".to_owned()));
    }
}
