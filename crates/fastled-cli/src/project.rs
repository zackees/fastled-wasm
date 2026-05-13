//! Project initialisation and sketch detection utilities.
//!
//! Ports the core detection and download logic from:
//! * `src/fastled/sketch.py`   — sketch directory detection
//! * `src/fastled/project_init.py` — GitHub archive download / example extraction
//!
//! Interactive prompts deliberately stay in Python; this module only provides
//! the pure-logic building blocks.

// Not every function is consumed by the CLI entry point yet.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

// ---------------------------------------------------------------------------
// Sketch detection
// ---------------------------------------------------------------------------

/// Return `true` when `dir` looks like a FastLED Arduino sketch directory.
///
/// A directory qualifies when it contains at least one of:
/// * A `.ino` file (Arduino sketch source)
/// * A `.cpp` file (plain C++ sketch variant)
/// * A `platformio.ini` manifest
///
/// Mirrors `sketch.py::looks_like_sketch_directory`.
pub fn is_sketch_dir(dir: &Path) -> bool {
    if !dir.is_dir() {
        return false;
    }

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return false,
    };

    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.ends_with(".ino") || name.ends_with(".cpp") || name == "platformio.ini" {
            return true;
        }
    }

    false
}

/// Return `true` when `directory` looks like a sketch directory using the
/// Python-side semantics.
pub fn looks_like_sketch_directory(directory: &Path, quick: bool) -> bool {
    if !directory.is_dir() || is_fastled_repo(directory) {
        return false;
    }

    if !quick && sketch_file_count_exceeds_limit(directory, 100) {
        return false;
    }

    is_sketch_dir(directory)
}

/// Find sketch directories inside `root`.
///
/// Behaviour mirrors `sketch.py::find_sketch_directories`:
/// * Scans one level deep for sketch directories.
/// * When a directory named `examples` is encountered it recurses up to three
///   levels deep (matching the Python recursive search).
/// * Hidden directories (names starting with `.`) are skipped.
/// * Stops after examining `MAX_ENTRIES` directory entries to prevent runaway
///   scanning on large trees.
///
/// Returns paths **relative** to `root`, sorted lexicographically.
pub fn find_sketches(root: &Path) -> Vec<PathBuf> {
    const MAX_ENTRIES: usize = 10_000;

    let mut results: Vec<PathBuf> = Vec::new();
    let mut count = 0usize;

    let top_entries = match fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return results,
    };

    for entry in top_entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_owned(),
            None => continue,
        };
        if name.starts_with('.') {
            continue;
        }

        count += 1;
        if count > MAX_ENTRIES {
            break;
        }

        if name.eq_ignore_ascii_case("examples") {
            // Recurse into examples/ up to three levels deep.
            _search_examples(&path, root, &mut results, &mut count, 0, 3);
        } else if looks_like_sketch_directory(&path, true) {
            if let Ok(rel) = path.strip_prefix(root) {
                results.push(rel.to_path_buf());
            }
        }
    }

    results.sort();
    results
}

/// Recursive helper used by `find_sketches` for the `examples/` subtree.
fn _search_examples(
    dir: &Path,
    root: &Path,
    results: &mut Vec<PathBuf>,
    count: &mut usize,
    depth: usize,
    max_depth: usize,
) {
    if depth >= max_depth {
        return;
    }

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_owned(),
            None => continue,
        };
        if name.starts_with('.') {
            continue;
        }

        *count += 1;
        if *count > 10_000 {
            return;
        }

        if looks_like_sketch_directory(&path, true) {
            if let Ok(rel) = path.strip_prefix(root) {
                results.push(rel.to_path_buf());
            }
        } else {
            // Continue searching deeper even if this level is not a sketch.
            _search_examples(&path, root, results, count, depth + 1, max_depth);
        }
    }
}

fn sketch_file_count_exceeds_limit(directory: &Path, limit: usize) -> bool {
    let mut count = 0usize;
    count_sketch_files(directory, limit, &mut count)
}

fn count_sketch_files(directory: &Path, limit: usize, count: &mut usize) -> bool {
    let entries = match fs::read_dir(directory) {
        Ok(e) => e,
        Err(_) => return false,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();

        if path.is_dir() {
            if name.starts_with('.') || name.contains("fastled_js") {
                continue;
            }
            if count_sketch_files(&path, limit, count) {
                return true;
            }
            continue;
        }

        if name.starts_with('.') || name.contains("platformio.ini") {
            continue;
        }

        *count += 1;
        if *count > limit {
            return true;
        }
    }

    false
}

// ---------------------------------------------------------------------------
// FastLED repo detection
// ---------------------------------------------------------------------------

/// Return `true` when `path` appears to be a FastLED library repository root.
///
/// Detection logic (first match wins):
/// 1. `library.properties` exists and contains the text `FastLED`.
/// 2. `src/FastLED.h` exists (typical Arduino library layout).
/// 3. `library.json` exists and its `name` field equals `"FastLED"`.
///
/// Mirrors the detection used in `sketch.py::looks_like_fastled_repo` and the
/// `_find_fastled_repo_via_library_json` walk in `project_init.py`.
pub fn is_fastled_repo(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }

    // Check library.properties (Arduino IDE format)
    let lib_props = path.join("library.properties");
    if lib_props.is_file() {
        if let Ok(txt) = fs::read_to_string(&lib_props) {
            if txt.contains("FastLED") {
                return true;
            }
        }
    }

    // Check src/FastLED.h (common header marker)
    if path.join("src").join("FastLED.h").is_file() {
        return true;
    }

    // Check library.json (PlatformIO format)
    let lib_json = path.join("library.json");
    if lib_json.is_file() {
        if let Ok(txt) = fs::read_to_string(&lib_json) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&txt) {
                if val.get("name").and_then(|v| v.as_str()) == Some("FastLED") {
                    return true;
                }
            }
        }
    }

    false
}

/// Walk upwards from `start` and return the first directory that looks like a
/// FastLED repository.
pub fn find_fastled_repo_upwards(start: &Path, max_depth: usize) -> Option<PathBuf> {
    let mut current = start.canonicalize().unwrap_or_else(|_| start.to_path_buf());

    for _ in 0..=max_depth {
        if is_fastled_repo(&current) {
            return Some(current);
        }

        let parent = current.parent()?.to_path_buf();
        if parent == current {
            break;
        }
        current = parent;
    }

    None
}

/// Collect available example names from a FastLED `examples/` directory.
pub fn collect_examples(examples_dir: &Path) -> Vec<String> {
    if !examples_dir.is_dir() {
        return Vec::new();
    }

    let mut found = Vec::new();
    let entries = match fs::read_dir(examples_dir) {
        Ok(e) => e,
        Err(_) => return found,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        if contains_ino_files(&path) {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                found.push(name.to_owned());
            }
            continue;
        }

        let nested_entries = match fs::read_dir(&path) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for nested in nested_entries.flatten() {
            let nested_path = nested.path();
            if !nested_path.is_dir() || !contains_ino_files(&nested_path) {
                continue;
            }
            if let Some(name) = nested_path.file_name().and_then(|n| n.to_str()) {
                found.push(name.to_owned());
            }
        }
    }

    found.sort();
    found
}

/// Read the `ref` field from `fastled.json` in `directory`.
pub fn read_fastled_json_ref(directory: &Path) -> Option<String> {
    let fpath = directory.join("fastled.json");
    let txt = fs::read_to_string(fpath).ok()?;
    let value: serde_json::Value = serde_json::from_str(&txt).ok()?;
    value.get("ref")?.as_str().map(str::to_owned)
}

/// Derive the resolved FastLED ref name from a cached repo directory.
///
/// `install::ensure_fastled_repo()` materializes repos under names like
/// `~/.fastled/cache/fastled-3.9.12/`, so we can recover the effective ref
/// from the leaf directory name for user-facing messages and `fastled.json`.
pub fn cached_repo_ref_name(repo_root: &Path) -> String {
    repo_root
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.strip_prefix("fastled-").unwrap_or(name))
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| "master".to_owned())
}

/// Write `fastled.json` with the provided `ref_name`, preserving any existing
/// keys when the file already contains valid JSON.
pub fn write_fastled_json_ref(directory: &Path, ref_name: &str) -> Result<()> {
    let fpath = directory.join("fastled.json");
    let mut data = if fpath.is_file() {
        fs::read_to_string(&fpath)
            .ok()
            .and_then(|txt| {
                serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&txt).ok()
            })
            .unwrap_or_default()
    } else {
        serde_json::Map::new()
    };

    data.insert(
        "ref".to_owned(),
        serde_json::Value::String(ref_name.to_owned()),
    );

    let mut json = serde_json::to_string_pretty(&serde_json::Value::Object(data))
        .context("serialize fastled.json")?;
    json.push('\n');
    fs::write(&fpath, json).with_context(|| format!("write {}", fpath.display()))?;
    Ok(())
}

fn contains_ino_files(directory: &Path) -> bool {
    let entries = match fs::read_dir(directory) {
        Ok(e) => e,
        Err(_) => return false,
    };

    entries.flatten().any(|entry| {
        entry.path().is_file()
            && entry
                .path()
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("ino"))
                .unwrap_or(false)
    })
}

// ---------------------------------------------------------------------------
// GitHub archive download and example extraction
// ---------------------------------------------------------------------------

const GITHUB_REPO: &str = "FastLED/FastLED";
const GITHUB_RELEASES_API: &str = "https://api.github.com/repos/FastLED/FastLED/releases/latest";

/// Fetch the latest release tag from the GitHub API.
///
/// Returns `None` when the request fails or the response cannot be parsed.
fn fetch_latest_release_tag() -> Option<String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()?;

    let resp = client
        .get(GITHUB_RELEASES_API)
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let body = resp.text().ok()?;
    let json: serde_json::Value = serde_json::from_str(&body).ok()?;
    json.get("tag_name")
        .and_then(|v: &serde_json::Value| v.as_str())
        .map(|s: &str| s.to_owned())
}

/// Return `true` when `ref_str` looks like a git commit SHA (7–40 hex chars).
fn is_commit_sha(ref_str: &str) -> bool {
    let len = ref_str.len();
    (7..=40).contains(&len) && ref_str.chars().all(|c| c.is_ascii_hexdigit())
}

/// Build the archive download URL for a branch/tag `ref_str`.
fn archive_url_for_ref(ref_str: &str) -> String {
    let base = format!("https://github.com/{GITHUB_REPO}/archive");
    if is_commit_sha(ref_str) {
        format!("{base}/{ref_str}.zip")
    } else {
        format!("{base}/refs/heads/{ref_str}.zip")
    }
}

/// Build the archive download URL for an explicit tag.
fn archive_url_for_tag(tag: &str) -> String {
    format!("https://github.com/{GITHUB_REPO}/archive/refs/tags/{tag}.zip")
}

/// Resolve an optional `branch` hint into `(display_name, archive_url)`.
///
/// * `None`  → fetch latest release tag; fall back to `master` on failure.
/// * `Some(s)` → use as a branch/tag/commit ref.
fn resolve_ref(branch: Option<&str>) -> (String, String) {
    match branch {
        None => {
            if let Some(tag) = fetch_latest_release_tag() {
                let url = archive_url_for_tag(&tag);
                return (tag, url);
            }
            eprintln!("Warning: could not fetch latest release, falling back to master");
            ("master".to_owned(), archive_url_for_ref("master"))
        }
        Some(r) => {
            let url = if is_commit_sha(r) {
                archive_url_for_ref(r)
            } else {
                archive_url_for_tag(r)
            };
            (r.to_owned(), url)
        }
    }
}

/// Download and extract the FastLED `example_name` sketch from GitHub.
///
/// Steps:
/// 1. Resolve the `branch` hint to an archive URL.
/// 2. Download the ZIP to a temporary file.
/// 3. Extract to a temporary directory.
/// 4. Locate the example inside `examples/` (flat or nested layout).
/// 5. Copy the example directory to `dest / example_name`.
///
/// Returns the path to the newly created sketch directory.
///
/// # Errors
/// Returns an error if the download, extraction, or copy fails, or if the
/// named example cannot be found in the archive.
pub fn init_example(example_name: &str, dest: &Path, branch: Option<&str>) -> Result<PathBuf> {
    let (_ref_name, url) = resolve_ref(branch);

    // Download the zip to a temp file.
    let tmp_dir = tempfile::tempdir().context("failed to create temp directory")?;
    let zip_path = tmp_dir.path().join("fastled.zip");

    crate::archive::download(&url, &zip_path)
        .with_context(|| format!("failed to download FastLED archive from {url}"))?;

    // Extract.
    let extract_dir = tmp_dir.path().join("extracted");
    crate::archive::extract_zip(&zip_path, &extract_dir)
        .context("failed to extract FastLED archive")?;

    // Find repo root (first dir starting with "FastLED").
    let repo_root = find_archive_repo_root(&extract_dir)?;

    // Locate the example.
    let example_src = find_example_in_repo(&repo_root, example_name)
        .with_context(|| format!("example '{example_name}' not found in FastLED archive"))?;

    // Copy to dest.
    fs::create_dir_all(dest)
        .with_context(|| format!("cannot create output directory {}", dest.display()))?;
    let out_path = dest.join(example_name);
    copy_dir_all(&example_src, &out_path)
        .with_context(|| format!("failed to copy example to {}", out_path.display()))?;

    Ok(out_path)
}

/// Copy `example_name` from an already-materialized FastLED repo into
/// `output_dir/example_name`, optionally persisting `ref_name` in
/// `fastled.json`.
pub fn init_example_from_repo(
    repo_root: &Path,
    example_name: &str,
    output_dir: &Path,
    ref_name: Option<&str>,
) -> Result<PathBuf> {
    let example_src = find_example_in_repo(repo_root, example_name).with_context(|| {
        format!(
            "example '{example_name}' not found in FastLED repo {}",
            repo_root.display()
        )
    })?;

    fs::create_dir_all(output_dir)
        .with_context(|| format!("cannot create output directory {}", output_dir.display()))?;
    let out_path = output_dir.join(example_name);
    copy_dir_all(&example_src, &out_path)
        .with_context(|| format!("failed to copy example to {}", out_path.display()))?;

    if let Some(ref_name) = ref_name {
        write_fastled_json_ref(&out_path, ref_name)?;
    }

    Ok(out_path)
}

/// Find the FastLED repo root inside an extracted archive directory.
///
/// The zip produces a top-level directory like `FastLED-master` or
/// `FastLED-3.9.12`; this function returns its path.
fn find_archive_repo_root(extract_dir: &Path) -> Result<PathBuf> {
    let entries: Vec<PathBuf> = fs::read_dir(extract_dir)
        .with_context(|| format!("cannot read {}", extract_dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("FastLED"))
                    .unwrap_or(false)
        })
        .collect();

    entries
        .into_iter()
        .next()
        .context("FastLED directory not found in downloaded archive")
}

/// Locate a named example inside an extracted FastLED repo.
///
/// Handles both layouts:
/// * `examples/{name}/`        — flat
/// * `examples/*/{name}/`      — one level of nesting (e.g. `examples/Fx/`)
pub fn find_example_in_repo(repo_root: &Path, name: &str) -> Option<PathBuf> {
    let examples_dir = repo_root.join("examples");
    if !examples_dir.is_dir() {
        return None;
    }

    // Flat: examples/{name}/
    let direct = examples_dir.join(name);
    if direct.is_dir() {
        return Some(direct);
    }

    // Nested: examples/*/{name}/
    for entry in fs::read_dir(&examples_dir).ok()?.flatten() {
        let sub = entry.path();
        if sub.is_dir() {
            let nested = sub.join(name);
            if nested.is_dir() {
                return Some(nested);
            }
        }
    }

    None
}

/// Recursively copy the directory `src` into `dest` (creates `dest`).
fn copy_dir_all(src: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest).with_context(|| format!("cannot create {}", dest.display()))?;

    for entry in fs::read_dir(src)
        .with_context(|| format!("cannot read {}", src.display()))?
        .flatten()
    {
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_all(&src_path, &dest_path)?;
        } else {
            fs::copy(&src_path, &dest_path).with_context(|| {
                format!(
                    "cannot copy {} to {}",
                    src_path.display(),
                    dest_path.display()
                )
            })?;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Fuzzy sketch name matching
// ---------------------------------------------------------------------------

/// Find the best-matching sketch name from `candidates` for a given `query`.
///
/// Uses the Jaro-Winkler distance (via `strsim`) to score each candidate.
/// Returns the candidate(s) with the highest score.
///
/// When `query` is an exact substring of a candidate that candidate is
/// returned immediately (mirrors the Python `partial_name in sketch_str`
/// fast-path in `sketch.py::find_sketch_by_partial_name`).
///
/// Returns an empty `Vec` when `candidates` is empty.
pub fn best_sketch_match(query: &str, candidates: &[&str]) -> Vec<String> {
    if candidates.is_empty() {
        return Vec::new();
    }

    let q_lower = query.to_lowercase();

    // Fast-path: exact substring match (case-insensitive).
    let substring_hits: Vec<&str> = candidates
        .iter()
        .copied()
        .filter(|c| c.to_lowercase().contains(&q_lower))
        .collect();
    if !substring_hits.is_empty() {
        return substring_hits.iter().map(|s| s.to_string()).collect();
    }

    // Fuzzy: Jaro-Winkler distance.
    let mut scored: Vec<(f64, &str)> = candidates
        .iter()
        .map(|c| (strsim::jaro_winkler(&q_lower, &c.to_lowercase()), *c))
        .collect();

    // Sort descending by score.
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let best_score = scored[0].0;
    // Return all candidates tied at the best score (within floating-point epsilon).
    scored
        .iter()
        .take_while(|(s, _)| (s - best_score).abs() < 1e-9)
        .map(|(_, name)| name.to_string())
        .collect()
}

/// Find a sketch directory by partial name match using the Python-side rules.
///
/// Returns a path relative to `search_dir`, mirroring `find_sketches`.
pub fn find_sketch_by_partial_name(partial_name: &str, search_dir: &Path) -> Result<PathBuf> {
    let sketch_directories = find_sketches(search_dir);
    let partial_name_normalized = partial_name.replace('\\', "/").to_lowercase();
    let partial_chars: std::collections::HashSet<char> = partial_name_normalized.chars().collect();

    let similarity = |candidate: &Path| -> f64 {
        if partial_chars.is_empty() {
            return 0.0;
        }

        let candidate_normalized = candidate
            .to_string_lossy()
            .replace('\\', "/")
            .to_lowercase();
        let candidate_chars: std::collections::HashSet<char> =
            candidate_normalized.chars().collect();
        let matching_chars = partial_chars.intersection(&candidate_chars).count();
        matching_chars as f64 / partial_chars.len() as f64
    };

    let matches: Vec<PathBuf> = sketch_directories
        .iter()
        .filter_map(|sketch_dir| {
            let sketch_str_normalized = sketch_dir
                .to_string_lossy()
                .replace('\\', "/")
                .to_lowercase();
            (sketch_str_normalized.contains(&partial_name_normalized)
                && similarity(sketch_dir) >= 0.5)
                .then_some(sketch_dir.clone())
        })
        .collect();

    if matches.is_empty() {
        let all_low_similarity = sketch_directories
            .iter()
            .all(|sketch_dir| similarity(sketch_dir) <= 0.5);

        if all_low_similarity && !sketch_directories.is_empty() {
            let sketches_str = sketch_directories
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("\n  ");
            bail!(
                "'{}' does not look like any of the available sketches.\n\nAvailable sketches:\n  {}",
                partial_name,
                sketches_str
            );
        }

        bail!("No sketch directory found matching '{}'", partial_name);
    }

    if matches.len() == 1 {
        return Ok(matches[0].clone());
    }

    let exact_matches: Vec<PathBuf> = matches
        .iter()
        .filter(|candidate| {
            candidate
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.eq_ignore_ascii_case(&partial_name_normalized))
                .unwrap_or(false)
        })
        .cloned()
        .collect();

    if exact_matches.len() == 1 {
        return Ok(exact_matches[0].clone());
    }

    let ambiguous_matches = if exact_matches.is_empty() {
        matches
    } else {
        exact_matches
    };
    let matches_str = ambiguous_matches
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("\n  ");
    bail!(
        "Multiple sketch directories found matching '{}':\n  {}",
        partial_name,
        matches_str
    );
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn temp_dir() -> TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    // ------------------------------------------------------------------
    // is_sketch_dir
    // ------------------------------------------------------------------

    #[test]
    fn test_is_sketch_dir_with_ino() {
        let dir = temp_dir();
        let sketch = dir.path().join("MySketch");
        fs::create_dir(&sketch).unwrap();
        fs::write(sketch.join("MySketch.ino"), b"void setup() {}").unwrap();
        assert!(is_sketch_dir(&sketch));
    }

    #[test]
    fn test_is_sketch_dir_with_cpp() {
        let dir = temp_dir();
        let sketch = dir.path().join("CppSketch");
        fs::create_dir(&sketch).unwrap();
        fs::write(sketch.join("main.cpp"), b"int main() {}").unwrap();
        assert!(is_sketch_dir(&sketch));
    }

    #[test]
    fn test_is_sketch_dir_with_platformio() {
        let dir = temp_dir();
        let sketch = dir.path().join("PioSketch");
        fs::create_dir(&sketch).unwrap();
        fs::write(sketch.join("platformio.ini"), b"[env]").unwrap();
        assert!(is_sketch_dir(&sketch));
    }

    #[test]
    fn test_is_sketch_dir_empty_dir() {
        let dir = temp_dir();
        let empty = dir.path().join("empty");
        fs::create_dir(&empty).unwrap();
        assert!(!is_sketch_dir(&empty));
    }

    #[test]
    fn test_is_sketch_dir_nonexistent() {
        let dir = temp_dir();
        assert!(!is_sketch_dir(&dir.path().join("does_not_exist")));
    }

    #[test]
    fn test_looks_like_sketch_directory_rejects_fastled_repo() {
        let dir = temp_dir();
        let root = dir.path();
        fs::write(root.join("library.properties"), b"name=FastLED\n").unwrap();
        fs::write(root.join("demo.ino"), b"void setup() {}").unwrap();
        assert!(!looks_like_sketch_directory(root, false));
    }

    #[test]
    fn test_looks_like_sketch_directory_rejects_large_non_quick_tree() {
        let dir = temp_dir();
        let sketch = dir.path().join("BigSketch");
        fs::create_dir(&sketch).unwrap();
        fs::write(sketch.join("BigSketch.ino"), b"void setup() {}").unwrap();
        for i in 0..101 {
            fs::write(sketch.join(format!("file{i}.txt")), b"x").unwrap();
        }
        assert!(!looks_like_sketch_directory(&sketch, false));
        assert!(looks_like_sketch_directory(&sketch, true));
    }

    // ------------------------------------------------------------------
    // find_sketches
    // ------------------------------------------------------------------

    #[test]
    fn test_find_sketches_basic() {
        let dir = temp_dir();
        let root = dir.path();

        // Create two sketch directories and one non-sketch directory.
        let a = root.join("Alpha");
        let b = root.join("Beta");
        let c = root.join("NotASketch");
        for d in [&a, &b, &c] {
            fs::create_dir(d).unwrap();
        }
        fs::write(a.join("Alpha.ino"), b"void setup() {}").unwrap();
        fs::write(b.join("Beta.ino"), b"void setup() {}").unwrap();
        // NotASketch has no qualifying files.

        let sketches = find_sketches(root);
        let names: Vec<&str> = sketches
            .iter()
            .map(|p| p.file_name().and_then(|n| n.to_str()).unwrap_or(""))
            .collect();

        assert!(names.contains(&"Alpha"), "Alpha should be found: {names:?}");
        assert!(names.contains(&"Beta"), "Beta should be found: {names:?}");
        assert!(
            !names.contains(&"NotASketch"),
            "NotASketch should not be found: {names:?}"
        );
    }

    #[test]
    fn test_find_sketches_in_examples() {
        let dir = temp_dir();
        let root = dir.path();

        let examples = root.join("examples");
        let ex1 = examples.join("Blink");
        let ex2 = examples.join("Fx").join("FxWave");
        for d in [&ex1, &ex2] {
            fs::create_dir_all(d).unwrap();
        }
        fs::write(ex1.join("Blink.ino"), b"void setup() {}").unwrap();
        fs::write(ex2.join("FxWave.ino"), b"void setup() {}").unwrap();

        let sketches = find_sketches(root);
        let names: Vec<String> = sketches
            .iter()
            .map(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_owned()
            })
            .collect();

        assert!(names.contains(&"Blink".to_owned()), "Blink: {names:?}");
        assert!(names.contains(&"FxWave".to_owned()), "FxWave: {names:?}");
    }

    #[test]
    fn test_find_sketches_hidden_dirs_skipped() {
        let dir = temp_dir();
        let root = dir.path();

        let hidden = root.join(".hidden");
        fs::create_dir(&hidden).unwrap();
        fs::write(hidden.join("sketch.ino"), b"void setup() {}").unwrap();

        let sketches = find_sketches(root);
        assert!(
            sketches.is_empty(),
            "hidden directory should be skipped: {sketches:?}"
        );
    }

    // ------------------------------------------------------------------
    // is_fastled_repo
    // ------------------------------------------------------------------

    #[test]
    fn test_is_fastled_repo_library_properties() {
        let dir = temp_dir();
        let root = dir.path();
        fs::write(
            root.join("library.properties"),
            b"name=FastLED\nversion=3.9.12\n",
        )
        .unwrap();
        assert!(is_fastled_repo(root));
    }

    #[test]
    fn test_is_fastled_repo_src_header() {
        let dir = temp_dir();
        let root = dir.path();
        let src = root.join("src");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("FastLED.h"), b"// FastLED header").unwrap();
        assert!(is_fastled_repo(root));
    }

    #[test]
    fn test_is_fastled_repo_library_json() {
        let dir = temp_dir();
        let root = dir.path();
        fs::write(root.join("library.json"), br#"{"name": "FastLED"}"#).unwrap();
        assert!(is_fastled_repo(root));
    }

    #[test]
    fn test_is_fastled_repo_negative() {
        let dir = temp_dir();
        let root = dir.path();
        // A plain sketch directory should not be mistaken for the FastLED repo.
        fs::write(root.join("sketch.ino"), b"void setup() {}").unwrap();
        assert!(!is_fastled_repo(root));
    }

    #[test]
    fn test_find_fastled_repo_upwards() {
        let dir = temp_dir();
        let root = dir.path().join("FastLED");
        let nested = root.join("examples").join("Blink");
        fs::create_dir_all(&nested).unwrap();
        fs::write(root.join("library.properties"), b"name=FastLED\n").unwrap();

        let found = find_fastled_repo_upwards(&nested, 10);
        let found = found.unwrap();
        assert_eq!(found.file_name().and_then(|n| n.to_str()), Some("FastLED"));
        assert!(found.ends_with("FastLED"));
    }

    #[test]
    fn test_collect_examples_handles_flat_and_nested_layouts() {
        let dir = temp_dir();
        let examples = dir.path().join("examples");
        let flat = examples.join("Blink");
        let nested = examples.join("Fx").join("FxWave");
        fs::create_dir_all(&flat).unwrap();
        fs::create_dir_all(&nested).unwrap();
        fs::write(flat.join("Blink.ino"), b"void setup() {}").unwrap();
        fs::write(nested.join("FxWave.ino"), b"void setup() {}").unwrap();

        let found = collect_examples(&examples);
        assert_eq!(found, vec!["Blink".to_owned(), "FxWave".to_owned()]);
    }

    #[test]
    fn test_read_and_write_fastled_json_ref() {
        let dir = temp_dir();
        write_fastled_json_ref(dir.path(), "master").unwrap();
        assert_eq!(read_fastled_json_ref(dir.path()).as_deref(), Some("master"));
    }

    #[test]
    fn test_cached_repo_ref_name_strips_fastled_prefix() {
        let path = Path::new("/tmp/fastled-3.9.12");
        assert_eq!(cached_repo_ref_name(path), "3.9.12");
    }

    #[test]
    fn test_cached_repo_ref_name_falls_back_when_name_missing() {
        let path = Path::new("");
        assert_eq!(cached_repo_ref_name(path), "master");
    }

    #[test]
    fn test_write_fastled_json_ref_preserves_other_fields() {
        let dir = temp_dir();
        fs::write(
            dir.path().join("fastled.json"),
            "{\n  \"name\": \"demo\"\n}\n",
        )
        .unwrap();

        write_fastled_json_ref(dir.path(), "3.9.12").unwrap();

        let txt = fs::read_to_string(dir.path().join("fastled.json")).unwrap();
        let value: serde_json::Value = serde_json::from_str(&txt).unwrap();
        assert_eq!(value.get("name").and_then(|v| v.as_str()), Some("demo"));
        assert_eq!(value.get("ref").and_then(|v| v.as_str()), Some("3.9.12"));
    }

    #[test]
    fn test_init_example_from_repo_copies_example_and_writes_ref() {
        let dir = temp_dir();
        let repo_root = dir.path().join("FastLED");
        let example = repo_root.join("examples").join("Blink");
        let out_root = dir.path().join("out");
        fs::create_dir_all(&example).unwrap();
        fs::write(example.join("Blink.ino"), b"void setup() {}").unwrap();

        let out = init_example_from_repo(&repo_root, "Blink", &out_root, Some("master")).unwrap();

        assert!(out.join("Blink.ino").is_file());
        assert_eq!(read_fastled_json_ref(&out).as_deref(), Some("master"));
    }

    // ------------------------------------------------------------------
    // best_sketch_match (fuzzy matching)
    // ------------------------------------------------------------------

    #[test]
    fn test_best_sketch_match_exact_substring() {
        let candidates = ["Blink", "BlinkFast", "Fire2012"];
        let matches = best_sketch_match("Blink", &candidates);
        assert!(
            matches.contains(&"Blink".to_owned()),
            "Blink should match: {matches:?}"
        );
        assert!(
            matches.contains(&"BlinkFast".to_owned()),
            "BlinkFast should match: {matches:?}"
        );
        assert!(
            !matches.contains(&"Fire2012".to_owned()),
            "Fire2012 should not match: {matches:?}"
        );
    }

    #[test]
    fn test_best_sketch_match_fuzzy_fallback() {
        let candidates = ["Blink", "Fire2012", "Noise"];
        let matches = best_sketch_match("blnk", &candidates);
        // Jaro-Winkler should rank "Blink" highest.
        assert!(!matches.is_empty(), "fuzzy match should return results");
        assert_eq!(matches[0], "Blink", "best fuzzy match should be Blink");
    }

    #[test]
    fn test_best_sketch_match_empty_candidates() {
        let matches = best_sketch_match("anything", &[]);
        assert!(matches.is_empty());
    }

    #[test]
    fn test_best_sketch_match_case_insensitive() {
        let candidates = ["Blink", "Fire2012"];
        let matches = best_sketch_match("BLINK", &candidates);
        assert!(
            matches.contains(&"Blink".to_owned()),
            "case-insensitive match: {matches:?}"
        );
    }

    #[test]
    fn test_find_sketch_by_partial_name_returns_unique_match() {
        let dir = temp_dir();
        let root = dir.path();
        let sketch = root.join("examples").join("FxWave2d");
        fs::create_dir_all(&sketch).unwrap();
        fs::write(sketch.join("FxWave2d.ino"), b"void setup() {}").unwrap();

        let matched = find_sketch_by_partial_name("FxWave2d", root).unwrap();
        assert_eq!(matched, PathBuf::from("examples").join("FxWave2d"));
    }

    #[test]
    fn test_find_sketch_by_partial_name_prefers_exact_leaf_match() {
        let dir = temp_dir();
        let root = dir.path();
        for name in ["sketch", "sketch1", "sketch2"] {
            let path = root.join("examples").join(name);
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join(format!("{name}.ino")), b"void setup() {}").unwrap();
        }

        let matched = find_sketch_by_partial_name("sketch", root).unwrap();
        assert_eq!(matched, PathBuf::from("examples").join("sketch"));
    }

    #[test]
    fn test_find_sketch_by_partial_name_reports_low_similarity_options() {
        let dir = temp_dir();
        let root = dir.path();
        for name in ["path/to/sketch", "examples/MyProject"] {
            let path = root.join(name);
            fs::create_dir_all(&path).unwrap();
            let file_name = path.file_name().and_then(|value| value.to_str()).unwrap();
            fs::write(path.join(format!("{file_name}.ino")), b"void setup() {}").unwrap();
        }

        let err = find_sketch_by_partial_name("blah", root)
            .unwrap_err()
            .to_string();
        assert!(err.contains("does not look like any of the available sketches"));
        assert!(err.contains("Available sketches:"));
        assert!(err.contains("sketch"));
        assert!(err.contains("MyProject"));
    }

    #[test]
    fn test_find_sketch_by_partial_name_reports_ambiguous_matches() {
        let dir = temp_dir();
        let root = dir.path();
        for name in ["examples/sketch1", "examples/sketch2"] {
            let path = root.join(name);
            fs::create_dir_all(&path).unwrap();
            let file_name = path.file_name().and_then(|value| value.to_str()).unwrap();
            fs::write(path.join(format!("{file_name}.ino")), b"void setup() {}").unwrap();
        }

        let err = find_sketch_by_partial_name("sketch", root)
            .unwrap_err()
            .to_string();
        assert!(err.contains("Multiple sketch directories found matching 'sketch'"));
        assert!(err.contains("sketch1"));
        assert!(err.contains("sketch2"));
    }
}
