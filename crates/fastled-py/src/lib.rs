use fastled_cli::install;
use fastled_cli::project;
use fastled_cli::{PromptChoice, SketchSelection};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyModule};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;
use zip::write::SimpleFileOptions;

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

fn normalize_optional_path(path: Option<&str>) -> Option<String> {
    path.map(PathBuf::from)
        .map(|value| normalize_path(&value).to_string_lossy().into_owned())
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

fn discover_artifacts(output_dir: &Path) -> HashMap<String, String> {
    let mut artifacts = HashMap::new();
    let candidates = [
        ("js", output_dir.join("fastled.js")),
        ("wasm", output_dir.join("fastled.wasm")),
        ("dwarf", output_dir.join("fastled.wasm.dwarf")),
        ("symbol_map", output_dir.join("fastled.js.symbols")),
        ("frontend_assets", output_dir.join("assets")),
    ];
    for (name, path) in candidates {
        if path.exists() {
            artifacts.insert(name.to_string(), path.to_string_lossy().into_owned());
        }
    }
    if !artifacts.contains_key("frontend_assets") && output_dir.exists() {
        artifacts.insert(
            "frontend_assets".to_string(),
            output_dir.to_string_lossy().into_owned(),
        );
    }
    artifacts
}

fn python_path(py: Python<'_>, path: &Path) -> PyResult<Py<PyAny>> {
    let pathlib = PyModule::import(py, "pathlib")?;
    let cls = pathlib.getattr("Path")?;
    Ok(cls
        .call1((path.to_string_lossy().as_ref(),))?
        .into_any()
        .unbind())
}

fn py_fspath(py: Python<'_>, value: &Bound<'_, PyAny>) -> PyResult<PathBuf> {
    let os = PyModule::import(py, "os")?;
    let fspath = os.getattr("fspath")?;
    let path_value = fspath.call1((value,))?;
    let path_str: String = path_value.extract()?;
    Ok(PathBuf::from(path_str))
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

#[pyclass(name = "NativeBuildService")]
struct NativeBuildService {
    toolchains: HashMap<Option<String>, Py<PyAny>>,
    states: HashMap<PathBuf, BuildState>,
}

#[pymethods]
impl NativeBuildService {
    #[new]
    fn new() -> Self {
        Self {
            toolchains: HashMap::new(),
            states: HashMap::new(),
        }
    }

    #[pyo3(signature = (toolchain, fastled_path=None))]
    fn register_toolchain(&mut self, toolchain: Py<PyAny>, fastled_path: Option<&str>) {
        let key = normalize_optional_path(fastled_path);
        self.toolchains.insert(key, toolchain);
    }

    #[pyo3(signature = (sketch_dir, build_mode, profile=false, fastled_path=None, force_clean=false))]
    fn detect_strategy(
        &mut self,
        sketch_dir: &str,
        build_mode: &str,
        profile: bool,
        fastled_path: Option<&str>,
        force_clean: bool,
    ) -> String {
        let sketch_dir = PathBuf::from(sketch_dir);
        self.detect_strategy_inner(
            &sketch_dir,
            build_mode,
            profile,
            normalize_optional_path(fastled_path),
            force_clean,
        )
    }

    #[pyo3(signature = (sketch_dir, build_mode, build_mode_obj, profile=false, fastled_path=None, force_clean=false))]
    fn build(
        &mut self,
        py: Python<'_>,
        sketch_dir: &str,
        build_mode: &str,
        build_mode_obj: Py<PyAny>,
        profile: bool,
        fastled_path: Option<&str>,
        force_clean: bool,
    ) -> PyResult<Py<PyDict>> {
        let sketch_dir = PathBuf::from(sketch_dir);
        let output_dir = sketch_dir.join("fastled_js");
        let fastled_path = normalize_optional_path(fastled_path);
        let strategy = self.detect_strategy_inner(
            &sketch_dir,
            build_mode,
            profile,
            fastled_path.clone(),
            force_clean,
        );

        if force_clean && output_dir.exists() {
            let _ = fs::remove_dir_all(&output_dir);
        }

        let toolchain = self
            .toolchains
            .get(&fastled_path)
            .map(|toolchain| toolchain.clone_ref(py))
            .ok_or_else(|| PyRuntimeError::new_err("No toolchain registered for build"))?;

        fs::create_dir_all(&output_dir).map_err(|err| PyRuntimeError::new_err(err.to_string()))?;

        let start = Instant::now();
        let result_payload = match self.run_toolchain(
            py,
            toolchain,
            &sketch_dir,
            &output_dir,
            build_mode_obj,
            profile,
        ) {
            Ok(js_file) => {
                let compile_time = start.elapsed().as_secs_f64();
                let zip_start = Instant::now();
                let zip_bytes = zip_output(&output_dir)
                    .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
                let zip_time = zip_start.elapsed().as_secs_f64();

                let state = BuildState {
                    build_mode: build_mode.to_string(),
                    profile,
                    fastled_path: fastled_path.clone(),
                };
                self.states
                    .insert(resolve_state_key(&sketch_dir), state.clone());
                write_state(&output_dir, &state);

                Self::build_payload(
                    py,
                    true,
                    format!(
                        "Native compilation successful!\nOutput: {}\nWASM: {}",
                        js_file.display(),
                        js_file.with_extension("wasm").display()
                    ),
                    zip_bytes,
                    zip_time,
                    compile_time,
                    strategy,
                    &output_dir,
                )?
            }
            Err(err) => {
                if err.is_instance_of::<pyo3::exceptions::PyKeyboardInterrupt>(py) {
                    return Err(err);
                }
                let compile_time = start.elapsed().as_secs_f64();
                Self::build_payload(
                    py,
                    false,
                    format!("Native compilation failed: {err}"),
                    Vec::new(),
                    0.0,
                    compile_time,
                    strategy,
                    &output_dir,
                )?
            }
        };

        Ok(result_payload)
    }

    fn purge(&mut self, sketch_dir: &str) {
        let sketch_dir = PathBuf::from(sketch_dir);
        self.states.remove(&resolve_state_key(&sketch_dir));
        let output_dir = sketch_dir.join("fastled_js");
        if output_dir.exists() {
            let _ = fs::remove_dir_all(output_dir);
        }
    }
}

impl NativeBuildService {
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

    fn run_toolchain(
        &self,
        py: Python<'_>,
        toolchain: Py<PyAny>,
        sketch_dir: &Path,
        output_dir: &Path,
        build_mode_obj: Py<PyAny>,
        profile: bool,
    ) -> PyResult<PathBuf> {
        let kwargs = PyDict::new(py);
        kwargs.set_item("sketch_dir", python_path(py, sketch_dir)?)?;
        kwargs.set_item("output_dir", python_path(py, output_dir)?)?;
        kwargs.set_item("build_mode", build_mode_obj)?;
        kwargs.set_item("profile", profile)?;

        let js_file = toolchain
            .bind(py)
            .call_method("compile", (), Some(&kwargs))?;
        py_fspath(py, &js_file)
    }

    fn build_payload(
        py: Python<'_>,
        success: bool,
        stdout: String,
        zip_bytes: Vec<u8>,
        zip_time: f64,
        sketch_time: f64,
        strategy: String,
        output_dir: &Path,
    ) -> PyResult<Py<PyDict>> {
        let payload = PyDict::new(py);
        let artifacts = PyDict::new(py);
        for (name, path) in discover_artifacts(output_dir) {
            artifacts.set_item(name, path)?;
        }

        payload.set_item("success", success)?;
        payload.set_item("stdout", stdout)?;
        payload.set_item("hash_value", py.None())?;
        payload.set_item("zip_bytes", PyBytes::new(py, &zip_bytes))?;
        payload.set_item("zip_time", zip_time)?;
        payload.set_item("libfastled_time", 0.0)?;
        payload.set_item("sketch_time", sketch_time)?;
        payload.set_item("response_processing_time", 0.0)?;
        payload.set_item("strategy", strategy)?;
        payload.set_item("output_dir", output_dir.to_string_lossy().into_owned())?;
        payload.set_item("artifacts", artifacts)?;

        Ok(payload.unbind())
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
    find_tauri_viewer_path().is_some()
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

/// FastLED native extension module.
#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<NativeBuildService>()?;
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_function(wrap_pyfunction!(watch_available, m)?)?;
    m.add_function(wrap_pyfunction!(archive_available, m)?)?;
    m.add_function(wrap_pyfunction!(project_available, m)?)?;
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
    m.add_function(wrap_pyfunction!(is_in_order_match, m)?)?;
    m.add_function(wrap_pyfunction!(string_diff, m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{is_in_order_match_impl, string_diff_impl};

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
