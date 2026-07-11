//! Frontend bundling: esbuild orchestration + frontend asset copy.
//!
//! Runs esbuild over `src/fastled/frontend/app.ts` and
//! `modules/core/fastled_background_worker.ts`, materialises the result under
//! `<frontend>/dist/`, then copies the contents of that dist directory into the
//! sketch output. A SHA-256 hash marker (`.frontend_hash`) lives at the output
//! root so repeat invocations skip the copy when nothing changed.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use crate::install;

// ---------------------------------------------------------------------------
// Source/dist resolution
// ---------------------------------------------------------------------------

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")))
}

/// Walk up from `CARGO_MANIFEST_DIR` looking for the workspace's
/// `src/fastled/frontend` directory.
fn default_source_dir() -> Result<PathBuf> {
    let candidate = workspace_root()
        .join("src")
        .join("fastled")
        .join("frontend");
    if candidate.is_dir() {
        return Ok(candidate);
    }
    // Fall back to walking parents (defensive: layout may shift).
    let mut current = Some(workspace_root());
    while let Some(dir) = current {
        let candidate = dir.join("src").join("fastled").join("frontend");
        if candidate.is_dir() {
            return Ok(candidate);
        }
        current = dir.parent().map(Path::to_path_buf);
    }
    anyhow::bail!(
        "could not locate src/fastled/frontend relative to CARGO_MANIFEST_DIR={}",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// Resolve the esbuild binary path.
///
/// Honours `FASTLED_ESBUILD_PATH` when set (so a Rust CLI run can re-use the
/// freshly-installed binary), otherwise falls back to
/// `install::ensure_esbuild_installed()`.
fn resolve_esbuild() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("FASTLED_ESBUILD_PATH") {
        let candidate = PathBuf::from(path);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    install::ensure_esbuild_installed().context("ensure esbuild installed")
}

// ---------------------------------------------------------------------------
// Filesystem helpers
// ---------------------------------------------------------------------------

/// Returns `true` when any component of `path` (relative to `root`) is
/// literally `dist`.
fn path_contains_dist_component(root: &Path, path: &Path) -> bool {
    let relative = path.strip_prefix(root).unwrap_or(path);
    relative
        .components()
        .any(|comp| matches!(comp, Component::Normal(name) if name == "dist"))
}

/// Recursively collect every regular file under `dir`, skipping any sub-tree
/// whose path contains a component named `dist`. Returns a sorted list of
/// absolute paths.
fn walk_files(dir: &Path) -> Vec<PathBuf> {
    fn inner(root: &Path, current: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(current) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path_contains_dist_component(root, &path) {
                continue;
            }
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                inner(root, &path, out);
            } else if file_type.is_file() {
                out.push(path);
            }
        }
    }

    let mut files = Vec::new();
    inner(dir, dir, &mut files);
    files.sort();
    files
}

/// Maximum mtime (seconds since UNIX epoch) over every non-`dist` file under
/// `source_dir`. Returns 0.0 when no files are found.
fn get_source_mtime(source_dir: &Path) -> Result<f64> {
    let mut max_mtime = 0.0f64;
    for path in walk_files(source_dir) {
        let meta = fs::metadata(&path).with_context(|| format!("stat {}", path.display()))?;
        let modified = meta
            .modified()
            .with_context(|| format!("modified mtime missing for {}", path.display()))?;
        let secs = modified
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        if secs > max_mtime {
            max_mtime = secs;
        }
    }
    Ok(max_mtime)
}

/// SHA-256 over a deterministic iteration of (relative path bytes, file bytes)
/// for every regular file under `dir`. Output is lower-case hex.
fn compute_dir_hash(dir: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut files: Vec<PathBuf> = Vec::new();
    fn collect(current: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(current) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                collect(&path, out);
            } else if file_type.is_file() {
                out.push(path);
            }
        }
    }
    collect(dir, &mut files);
    files.sort();

    for file_path in &files {
        let relative = file_path.strip_prefix(dir).unwrap_or(file_path);
        // Normalise separators so cross-platform hashes match.
        let relative_str = relative.to_string_lossy().replace('\\', "/");
        hasher.update(relative_str.as_bytes());

        let mut file =
            File::open(file_path).with_context(|| format!("open {}", file_path.display()))?;
        let mut buf = vec![0u8; 1024 * 64];
        loop {
            let n = file
                .read(&mut buf)
                .with_context(|| format!("read {}", file_path.display()))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
    }

    Ok(format!("{:x}", hasher.finalize()))
}

fn copy_file(src: &Path, dst: &Path) -> Result<()> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create parent {}", parent.display()))?;
    }
    fs::copy(src, dst).with_context(|| format!("copy {} -> {}", src.display(), dst.display()))?;
    Ok(())
}

fn copy_tree(src: &Path, dst: &Path) -> Result<()> {
    if dst.exists() {
        fs::remove_dir_all(dst)
            .with_context(|| format!("remove existing dst {}", dst.display()))?;
    }
    fs::create_dir_all(dst).with_context(|| format!("create dst {}", dst.display()))?;
    for entry in fs::read_dir(src).with_context(|| format!("read_dir {}", src.display()))? {
        let entry = entry?;
        let path = entry.path();
        let target = dst.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_tree(&path, &target)?;
        } else if file_type.is_file() {
            copy_file(&path, &target)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// esbuild orchestration
// ---------------------------------------------------------------------------

fn run_esbuild(esbuild: &Path, source_dir: &Path, args: &[String]) -> Result<()> {
    let status = Command::new(esbuild)
        .arg("--alias:three=./vendor/three/build/three.module.js")
        .args(args)
        .current_dir(source_dir)
        .status()
        .with_context(|| format!("spawn esbuild at {}", esbuild.display()))?;
    if !status.success() {
        anyhow::bail!(
            "esbuild failed with exit code {}",
            status.code().unwrap_or(-1)
        );
    }
    Ok(())
}

fn build_dist(source_dir: &Path) -> Result<PathBuf> {
    let dist_dir = source_dir.join("dist");
    let marker = dist_dir.join(".esbuild_marker");
    let source_mtime = get_source_mtime(source_dir)?;

    if dist_dir.exists()
        && marker.exists()
        // The application entry artifact was renamed from app.js to index.js.
        // Force one rebuild when an older cached dist is present.
        && dist_dir.join("index.js").is_file()
        && !dist_dir.join("app.js").exists()
    {
        if let Ok(text) = fs::read_to_string(&marker) {
            if let Ok(value) = text.trim().parse::<f64>() {
                if value >= source_mtime {
                    return Ok(dist_dir);
                }
            }
        }
    }

    if dist_dir.exists() {
        fs::remove_dir_all(&dist_dir)
            .with_context(|| format!("remove dist {}", dist_dir.display()))?;
    }
    fs::create_dir_all(&dist_dir).with_context(|| format!("create dist {}", dist_dir.display()))?;

    let esbuild = resolve_esbuild()?;

    let app_ts = source_dir.join("app.ts");
    let app_out = dist_dir.join("index.js");
    run_esbuild(
        &esbuild,
        source_dir,
        &[
            app_ts.to_string_lossy().into_owned(),
            "--bundle".to_string(),
            "--format=esm".to_string(),
            "--platform=browser".to_string(),
            "--target=es2021".to_string(),
            "--sourcemap".to_string(),
            format!("--outfile={}", app_out.display()),
            "--log-level=warning".to_string(),
        ],
    )?;

    let worker_ts = source_dir
        .join("modules")
        .join("core")
        .join("fastled_background_worker.ts");
    let worker_out = dist_dir.join("fastled_background_worker.js");
    run_esbuild(
        &esbuild,
        source_dir,
        &[
            worker_ts.to_string_lossy().into_owned(),
            "--bundle".to_string(),
            "--format=esm".to_string(),
            "--platform=browser".to_string(),
            "--target=es2021".to_string(),
            "--sourcemap".to_string(),
            format!("--outfile={}", worker_out.display()),
            "--log-level=warning".to_string(),
        ],
    )?;

    let index_src = source_dir.join("index.html");
    let index_html = fs::read_to_string(&index_src)
        .with_context(|| format!("read {}", index_src.display()))?
        .replace("./app.ts", "./index.js");
    fs::write(dist_dir.join("index.html"), index_html)
        .with_context(|| format!("write {}", dist_dir.join("index.html").display()))?;

    copy_file(&source_dir.join("index.css"), &dist_dir.join("index.css"))?;
    copy_file(
        &source_dir
            .join("modules")
            .join("audio")
            .join("audio_worklet_processor.js"),
        &dist_dir.join("audio_worklet_processor.js"),
    )?;

    let assets_dir = source_dir.join("assets");
    if assets_dir.exists() {
        copy_tree(&assets_dir, &dist_dir.join("assets"))?;
    }

    fs::write(&marker, format!("{}", source_mtime))
        .with_context(|| format!("write marker {}", marker.display()))?;
    Ok(dist_dir)
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Build the frontend (if stale) and copy the bundled output into `output_dir`.
///
/// Mirrors `src/fastled/frontend_esbuild.py::copy_frontend_to_output`. When
/// `source_dir` is `None`, the frontend tree is located relative to the
/// workspace `CARGO_MANIFEST_DIR`.
pub fn copy_frontend_to_output(output_dir: &Path, source_dir: Option<&Path>) -> Result<()> {
    let source = match source_dir {
        Some(path) => path.to_path_buf(),
        None => default_source_dir()?,
    };
    let dist_dir = build_dist(&source)?;

    let hash_marker = output_dir.join(".frontend_hash");
    let current_hash = compute_dir_hash(&dist_dir)?;
    if hash_marker.is_file() {
        if let Ok(existing) = fs::read_to_string(&hash_marker) {
            if existing.trim() == current_hash {
                println!("  Frontend assets unchanged, skipping copy.");
                return Ok(());
            }
        }
    }

    fs::create_dir_all(output_dir)
        .with_context(|| format!("create output dir {}", output_dir.display()))?;

    for entry in
        fs::read_dir(&dist_dir).with_context(|| format!("read_dir {}", dist_dir.display()))?
    {
        let entry = entry?;
        let destination = output_dir.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_tree(&entry.path(), &destination)?;
        } else {
            copy_file(&entry.path(), &destination)?;
        }
    }

    fs::write(&hash_marker, current_hash)
        .with_context(|| format!("write hash marker {}", hash_marker.display()))?;
    Ok(())
}

/// Remove only the default application's generated files from an output
/// directory. WASM, loader, debug, and sketch-asset artifacts are preserved.
pub fn remove_app_from_output(output_dir: &Path) -> Result<()> {
    for name in [
        "index.js",
        "index.js.map",
        "app.js",
        "app.js.map",
        "index.html",
        "index.css",
        "fastled_background_worker.js",
        "fastled_background_worker.js.map",
        "audio_worklet_processor.js",
        "emscripten.d.ts",
        "types.d.ts",
        "jsconfig.json",
        ".esbuild_marker",
        ".frontend_hash",
    ] {
        let path = output_dir.join(name);
        if path.is_file() {
            fs::remove_file(&path)
                .with_context(|| format!("remove stale app artifact {}", path.display()))?;
        }
    }
    let assets = output_dir.join("assets");
    if assets.is_dir() {
        fs::remove_dir_all(&assets)
            .with_context(|| format!("remove stale app assets {}", assets.display()))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;
    use tempfile::TempDir;

    fn write(path: &Path, content: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    #[test]
    fn compute_dir_hash_is_deterministic() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        // Create files in mixed order; the function must sort internally.
        write(&root.join("b.txt"), b"bravo");
        write(&root.join("a.txt"), b"alpha");
        write(&root.join("sub").join("c.txt"), b"charlie");

        let h1 = compute_dir_hash(root).unwrap();
        // Slight pause so any mtime-based caching wouldn't pollute the hash;
        // compute_dir_hash itself only hashes content, but be defensive.
        thread::sleep(Duration::from_millis(5));
        let h2 = compute_dir_hash(root).unwrap();
        assert_eq!(h1, h2, "hash must be deterministic across calls");
    }

    #[test]
    fn compute_dir_hash_changes_on_content_change() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        write(&root.join("a.txt"), b"alpha");

        let h1 = compute_dir_hash(root).unwrap();
        write(&root.join("a.txt"), b"alpha!");
        let h2 = compute_dir_hash(root).unwrap();
        assert_ne!(h1, h2, "hash must differ when a file body changes");
    }

    #[test]
    fn walk_files_skips_dist_subtree() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        write(&root.join("app.ts"), b"// app");
        write(&root.join("dist").join("index.js"), b"// built");
        write(&root.join("modules").join("util.ts"), b"// util");
        write(&root.join("mydist").join("note.txt"), b"// kept");

        let files = walk_files(root);
        let names: Vec<String> = files
            .iter()
            .map(|p| {
                p.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        assert!(names.contains(&"app.ts".to_string()));
        assert!(names.contains(&"modules/util.ts".to_string()));
        assert!(
            names.contains(&"mydist/note.txt".to_string()),
            "must not skip dirs that merely contain 'dist' in name"
        );
        assert!(
            !names.iter().any(|n| n.starts_with("dist/")),
            "must skip the dist/ subtree, got {names:?}"
        );
    }

    #[test]
    fn copy_frontend_to_output_skips_when_marker_matches() {
        // Stand up a fake dist tree, point copy_frontend_to_output at a source
        // that *already* has a populated dist + matching marker, so build_dist
        // returns immediately. Then run copy twice and confirm the second
        // invocation short-circuits on the hash marker.
        let workspace = TempDir::new().unwrap();
        let source = workspace.path().join("frontend");
        let dist = source.join("dist");
        fs::create_dir_all(&dist).unwrap();
        write(&dist.join("index.js"), b"// stub app");
        write(&dist.join("index.html"), b"<!doctype html><title>t</title>");
        // Marker must satisfy `marker_value >= source_mtime` so build_dist is a
        // no-op. Use a value far in the future so any reasonable source mtime
        // is <= it.
        write(&dist.join(".esbuild_marker"), b"9999999999.0");

        let output = TempDir::new().unwrap();
        copy_frontend_to_output(output.path(), Some(&source)).expect("first copy");
        assert!(output.path().join("index.js").exists());
        let hash_after_first = fs::read_to_string(output.path().join(".frontend_hash")).unwrap();

        // Touch nothing; second call must be a no-op (marker matches).
        let app_js_meta_before = fs::metadata(output.path().join("index.js"))
            .unwrap()
            .modified()
            .unwrap();
        thread::sleep(Duration::from_millis(20));
        copy_frontend_to_output(output.path(), Some(&source)).expect("second copy");
        let app_js_meta_after = fs::metadata(output.path().join("index.js"))
            .unwrap()
            .modified()
            .unwrap();
        let hash_after_second = fs::read_to_string(output.path().join(".frontend_hash")).unwrap();

        assert_eq!(
            hash_after_first, hash_after_second,
            "hash marker must be stable across no-op copies"
        );
        assert_eq!(
            app_js_meta_before, app_js_meta_after,
            "files must not be re-copied when the hash marker already matches"
        );
    }

    #[test]
    fn remove_app_from_output_preserves_runtime_and_assets_manifest() {
        let output = TempDir::new().unwrap();
        write(&output.path().join("index.js"), b"app");
        write(&output.path().join("index.html"), b"html");
        write(&output.path().join("index.css"), b"css");
        write(&output.path().join("fastled.js"), b"loader");
        write(&output.path().join("fastled.wasm"), b"wasm");
        write(&output.path().join("sketch_assets.json"), b"[]");
        write(&output.path().join("assets").join("font.ttf"), b"font");

        remove_app_from_output(output.path()).expect("remove app artifacts");

        assert!(!output.path().join("index.js").exists());
        assert!(!output.path().join("index.html").exists());
        assert!(!output.path().join("index.css").exists());
        assert!(!output.path().join("assets").exists());
        assert!(output.path().join("fastled.js").exists());
        assert!(output.path().join("fastled.wasm").exists());
        assert!(output.path().join("sketch_assets.json").exists());
    }
}
