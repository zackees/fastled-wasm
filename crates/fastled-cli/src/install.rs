//! Toolchain install entry points driven from the Rust CLI.
//!
//! Mirrors `src/fastled/toolchain/emscripten_archive.py` so the Python side
//! no longer needs `httpx` / `pyzstd` to materialise the emscripten toolchain.
//! Public entry points are intended to be called once at the top of the
//! compile flow; results are cached on disk via a `done.txt` marker.

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use ctcb_core::Target;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::archive;

/// Toolchain platform manifest as published in
/// `clang-tool-chain-bins/assets/emscripten/{platform}/{arch}/manifest.json`.
///
/// Local mirror of the schema rather than relying on `ctcb-manifest` because
/// the published manifests use a `"versions": { … }` map, while older
/// `ctcb-manifest` releases expected version keys at the top level. Keeping
/// the schema local insulates the CLI from upstream crate drift.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PlatformManifest {
    latest: String,
    versions: BTreeMap<String, VersionInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VersionInfo {
    href: String,
    sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parts: Option<Vec<PartRef>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PartRef {
    href: String,
    sha256: String,
}

const EMSCRIPTEN_MANIFEST_BASE_URL: &str =
    "https://raw.githubusercontent.com/zackees/clang-tool-chain-bins/main/assets/emscripten";

const ESBUILD_VERSION: &str = "0.28.0";

const FASTLED_REPO: &str = "FastLED/FastLED";
const FASTLED_LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/FastLED/FastLED/releases/latest";

fn fastled_root() -> Result<PathBuf> {
    let home = dirs::home_dir().context("cannot resolve home directory")?;
    Ok(home.join(".fastled"))
}

fn detect_platform_arch() -> Result<(String, String)> {
    let target = Target::current().context("detect host clang-tool-chain target")?;
    Ok((target.platform.to_string(), target.arch.to_string()))
}

/// Fetch and parse the platform manifest JSON.
fn fetch_manifest(url: &str) -> Result<PlatformManifest> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .context("failed to build HTTP client for manifest fetch")?;
    let response = client
        .get(url)
        .send()
        .with_context(|| format!("GET {url} failed"))?
        .error_for_status()
        .with_context(|| format!("manifest fetch returned error for {url}"))?;
    let text = response.text().context("read manifest body")?;
    parse_platform_manifest(&text).with_context(|| format!("parse manifest JSON from {url}"))
}

/// Parse a platform manifest, accepting both the current schema (`versions`
/// sub-map) and the historical layout where version keys live at the top
/// level next to `latest`. The two formats coexist on the assets server
/// today, so the CLI has to handle both.
fn parse_platform_manifest(text: &str) -> Result<PlatformManifest> {
    if let Ok(parsed) = serde_json::from_str::<PlatformManifest>(text) {
        return Ok(parsed);
    }

    let value: serde_json::Value = serde_json::from_str(text)?;
    let object = value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("manifest is not a JSON object"))?;

    let latest = object
        .get("latest")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("manifest is missing required field `latest`"))?
        .to_string();

    let mut versions = BTreeMap::new();
    for (key, value) in object {
        if key == "latest" {
            continue;
        }
        let info: VersionInfo = serde_json::from_value(value.clone())
            .with_context(|| format!("parse version entry `{key}`"))?;
        versions.insert(key.clone(), info);
    }

    if versions.is_empty() {
        bail!("manifest does not declare any versions");
    }
    if !versions.contains_key(&latest) {
        bail!("manifest `latest`={latest} has no matching version entry");
    }

    Ok(PlatformManifest { latest, versions })
}

/// Download each part to `parts_dir`, verify its SHA256, then concatenate the
/// parts into `merged_path`. Existing parts whose checksum still matches are
/// reused (no re-download).
fn download_multipart(parts: &[PartRef], parts_dir: &Path, merged_path: &Path) -> Result<()> {
    fs::create_dir_all(parts_dir)
        .with_context(|| format!("create parts dir {}", parts_dir.display()))?;
    let merged_file = fs::File::create(merged_path)
        .with_context(|| format!("create merged archive {}", merged_path.display()))?;
    let mut merged_writer = BufWriter::new(merged_file);

    for (index, part) in parts.iter().enumerate() {
        let part_path = parts_dir.join(format!("part-{:02}", index));
        if !part_path.exists() {
            archive::download(&part.href, &part_path)
                .with_context(|| format!("download part {}", part.href))?;
        }
        if !archive::verify_sha256(&part_path, &part.sha256)? {
            let actual = archive::sha256_file(&part_path).unwrap_or_default();
            let _ = fs::remove_file(&part_path);
            bail!(
                "checksum mismatch for {}: got {}, expected {}",
                part.href,
                actual,
                part.sha256
            );
        }
        let mut part_reader = BufReader::new(
            fs::File::open(&part_path)
                .with_context(|| format!("open part {}", part_path.display()))?,
        );
        std::io::copy(&mut part_reader, &mut merged_writer).with_context(|| {
            format!(
                "concatenate part {} into {}",
                part_path.display(),
                merged_path.display()
            )
        })?;
    }
    merged_writer.flush()?;
    Ok(())
}

/// Ensure the emscripten toolchain is installed at
/// `~/.fastled/toolchains/emscripten/{platform}/{arch}/{version}/`.
/// Returns the install directory path.
///
/// The layout intentionally matches the Python implementation so a previously
/// Python-installed toolchain is picked up without re-downloading.
pub fn ensure_emscripten_installed() -> Result<PathBuf> {
    let (platform, arch) = detect_platform_arch()?;
    let root = fastled_root()?;
    let install_base = root
        .join("toolchains")
        .join("emscripten")
        .join(&platform)
        .join(&arch);
    let cache_dir = root.join("toolchains").join("archives");
    fs::create_dir_all(&install_base)?;
    fs::create_dir_all(&cache_dir)?;

    let manifest_url = format!("{EMSCRIPTEN_MANIFEST_BASE_URL}/{platform}/{arch}/manifest.json");
    let manifest = fetch_manifest(&manifest_url)?;
    let version = manifest.latest.clone();
    let install_dir = install_base.join(&version);
    let done_file = install_dir.join("done.txt");
    if done_file.exists() {
        return Ok(install_dir);
    }

    let version_info = manifest
        .versions
        .get(&version)
        .with_context(|| format!("manifest has no entry for version {version}"))?;

    let archive_path = cache_dir.join(format!("emscripten-{platform}-{arch}-{version}.tar.zst"));
    let parts_subdir = cache_dir.join(format!("emscripten-{platform}-{arch}-{version}.parts"));

    if !archive_path.exists() {
        let parts = version_info
            .parts
            .as_ref()
            .with_context(|| format!("manifest version {version} has no parts"))?;
        download_multipart(parts, &parts_subdir, &archive_path)?;
    }

    if !archive::verify_sha256(&archive_path, &version_info.sha256)? {
        let actual = archive::sha256_file(&archive_path).unwrap_or_default();
        let _ = fs::remove_file(&archive_path);
        bail!(
            "archive checksum mismatch for {}: got {}, expected {}",
            version_info.href,
            actual,
            version_info.sha256
        );
    }

    // Atomic-ish install via staging directory.
    let staging = install_base.join(format!("{version}.staging"));
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;
    archive::extract_tar_zst(&archive_path, &staging)?;

    fs::write(staging.join("done.txt"), "ok\n")?;

    if install_dir.exists() {
        fs::remove_dir_all(&install_dir)?;
    }
    fs::rename(&staging, &install_dir)?;

    archive::write_emscripten_config(&install_dir, "node")?;

    Ok(install_dir)
}

// ---------------------------------------------------------------------------
// esbuild
// ---------------------------------------------------------------------------

/// npm package platform-arch strings differ from the emscripten ones.
fn esbuild_platform_arch() -> Result<(&'static str, &'static str)> {
    let platform = if cfg!(target_os = "windows") {
        "win32"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    };
    let arch = if cfg!(target_arch = "x86_64") {
        "x64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        anyhow::bail!(
            "unsupported architecture for esbuild: {}",
            std::env::consts::ARCH
        );
    };
    Ok((platform, arch))
}

/// Ensure the esbuild binary is installed at
/// `~/.fastled/toolchains/esbuild/{platform}/{arch}/{version}/`.
/// Returns the path to the executable.
///
/// Used by `crates/fastled-cli/src/frontend.rs` to bundle frontend assets.
pub fn ensure_esbuild_installed() -> Result<PathBuf> {
    let (platform, arch) = esbuild_platform_arch()?;
    let version = ESBUILD_VERSION;
    let root = fastled_root()?;
    let install_dir = root
        .join("toolchains")
        .join("esbuild")
        .join(platform)
        .join(arch)
        .join(version);
    let exe_name = if cfg!(target_os = "windows") {
        "esbuild.exe"
    } else {
        "esbuild"
    };
    let esbuild_path = install_dir.join(exe_name);
    let done_file = install_dir.join("done.txt");
    if done_file.exists() && esbuild_path.exists() {
        return Ok(esbuild_path);
    }
    fs::create_dir_all(&install_dir)?;

    let archive_cache = root.join("toolchains").join("archives");
    fs::create_dir_all(&archive_cache)?;
    let archive_path = archive_cache.join(format!("esbuild-{platform}-{arch}-{version}.tgz"));
    if !archive_path.exists() {
        let url = format!(
            "https://registry.npmjs.org/@esbuild/{platform}-{arch}/-/{platform}-{arch}-{version}.tgz"
        );
        archive::download(&url, &archive_path)?;
    }

    let member = if cfg!(target_os = "windows") {
        format!("package/{exe_name}")
    } else {
        "package/bin/esbuild".to_string()
    };

    if esbuild_path.exists() {
        let _ = fs::remove_file(&esbuild_path);
    }
    archive::extract_member_from_tgz(&archive_path, &member, &esbuild_path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&esbuild_path)?.permissions();
        perms.set_mode(perms.mode() | 0o111);
        fs::set_permissions(&esbuild_path, perms)?;
    }

    fs::write(&done_file, "ok\n")?;
    Ok(esbuild_path)
}

// ---------------------------------------------------------------------------
// FastLED repo download (used by --init)
// ---------------------------------------------------------------------------

fn is_commit_sha(ref_str: &str) -> bool {
    let n = ref_str.len();
    (7..=40).contains(&n) && ref_str.chars().all(|c| c.is_ascii_hexdigit())
}

/// Hit the GitHub API for the latest FastLED release tag.
/// Returns `None` on any failure so callers can fall back to `master`.
fn fetch_latest_release_tag() -> Option<String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .ok()?;
    let resp = client
        .get(FASTLED_LATEST_RELEASE_API)
        .header("Accept", "application/vnd.github.v3+json")
        .header("User-Agent", "fastled-cli")
        .send()
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let text = resp.text().ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn head_check(url: &str) -> bool {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build();
    match client {
        Ok(c) => c
            .head(url)
            .header("User-Agent", "fastled-cli")
            .send()
            .map(|r| r.status().is_success())
            .unwrap_or(false),
        Err(_) => false,
    }
}

/// Resolve `ref` to `(display_name, archive_url)`. Mirrors
/// `project_init._resolve_ref` in Python.
fn resolve_fastled_ref(ref_opt: Option<&str>) -> (String, String) {
    let archive_base = format!("https://github.com/{FASTLED_REPO}/archive");

    match ref_opt {
        None | Some("latest_release") => match fetch_latest_release_tag() {
            Some(tag) => {
                let url = format!("{archive_base}/refs/tags/{tag}.zip");
                (tag, url)
            }
            None => {
                eprintln!(
                    "fastled: could not fetch latest FastLED release tag, falling back to master"
                );
                let url = format!("{archive_base}/refs/heads/master.zip");
                ("master".to_string(), url)
            }
        },
        Some(r) if is_commit_sha(r) => {
            let url = format!("{archive_base}/{r}.zip");
            (r.to_string(), url)
        }
        Some(r) => {
            // Try as tag first, fall back to branch (mirrors Python).
            let tag_url = format!("{archive_base}/refs/tags/{r}.zip");
            if head_check(&tag_url) {
                (r.to_string(), tag_url)
            } else {
                let branch_url = format!("{archive_base}/refs/heads/{r}.zip");
                (r.to_string(), branch_url)
            }
        }
    }
}

/// Locate the root of an extracted FastLED archive (e.g. `FastLED-master`,
/// `FastLED-3.9.12`, `FastLED-<sha>`).
fn find_fastled_extract_root(dir: &Path) -> Result<PathBuf> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() && entry.file_name().to_string_lossy().starts_with("FastLED")
        {
            return Ok(entry.path());
        }
    }
    anyhow::bail!("no FastLED* directory found inside {}", dir.display())
}

/// Ensure the FastLED repo for `ref_opt` is downloaded and extracted under
/// `~/.fastled/cache/fastled-{ref}/`. Returns the resolved local repo root.
///
/// Re-uses an existing extraction if `library.json` is already present, so
/// repeated calls are cheap.
pub fn ensure_fastled_repo(ref_opt: Option<&str>) -> Result<PathBuf> {
    let (ref_name, url) = resolve_fastled_ref(ref_opt);
    let root = fastled_root()?;
    let cache_base = root.join("cache");
    fs::create_dir_all(&cache_base)?;
    let repo_dir = cache_base.join(format!("fastled-{ref_name}"));

    if repo_dir.join("library.json").is_file() {
        return Ok(repo_dir);
    }

    let archive_cache = cache_base.join("archives");
    fs::create_dir_all(&archive_cache)?;
    let archive_path = archive_cache.join(format!("FastLED-{ref_name}.zip"));
    if !archive_path.exists() {
        archive::download(&url, &archive_path)
            .with_context(|| format!("download FastLED archive from {url}"))?;
    }

    // Extract to a staging dir so a partial extraction never poisons the final
    // location.
    let staging = cache_base.join(format!("fastled-{ref_name}.staging"));
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;
    archive::extract_zip(&archive_path, &staging)?;

    let extracted_root = find_fastled_extract_root(&staging)?;

    if repo_dir.exists() {
        fs::remove_dir_all(&repo_dir)?;
    }
    fs::rename(&extracted_root, &repo_dir)?;
    fs::remove_dir_all(&staging).ok();

    Ok(repo_dir)
}

// ---------------------------------------------------------------------------
// User-facing install flow
// ---------------------------------------------------------------------------

const DEFAULT_INSTALL_EXAMPLE: &str = "wasm";
const AUTO_DEBUG_VSIX_URL: &str =
    "https://github.com/zackees/vscode-auto-debug/releases/latest/download/auto-debug.vsix";

#[derive(Clone, Copy, Debug)]
pub struct InstallOptions {
    pub dry_run: bool,
    pub no_interactive: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct InstallOutcome {
    pub launch_after: bool,
}

fn prompt_yes_no(prompt: &str, default: bool) -> Result<bool> {
    let default_hint = if default { "[Y/n]" } else { "[y/N]" };
    print!("{prompt} {default_hint} ");
    std::io::stdout().flush().context("flush prompt")?;

    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .context("read prompt response")?;
    let trimmed = input.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return Ok(default);
    }
    Ok(matches!(trimmed.as_str(), "y" | "yes"))
}

fn command_exists(command: &str) -> bool {
    Command::new(command)
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn detect_supported_ide() -> Option<(&'static str, &'static str)> {
    if command_exists("code") {
        Some(("code", "VSCode"))
    } else if command_exists("cursor") {
        Some(("cursor", "Cursor"))
    } else {
        None
    }
}

fn find_vscode_project_upward(max_levels: usize) -> Option<PathBuf> {
    let mut current = std::env::current_dir().ok()?;
    for _ in 0..max_levels {
        let parent = current.parent()?.to_path_buf();
        if parent == current {
            break;
        }
        current = parent;
        if current.join(".vscode").is_dir() {
            return Some(current);
        }
    }
    None
}

fn generate_vscode_project() -> Result<()> {
    let vscode_dir = std::env::current_dir()
        .context("current dir")?
        .join(".vscode");
    fs::create_dir_all(&vscode_dir).with_context(|| format!("create {}", vscode_dir.display()))?;
    println!("Created .vscode directory at {}", vscode_dir.display());
    Ok(())
}

fn validate_vscode_project(no_interactive: bool) -> Result<()> {
    let current_dir = std::env::current_dir().context("current dir")?;
    if current_dir.join(".vscode").is_dir() {
        return Ok(());
    }

    if let Some(parent_path) = find_vscode_project_upward(5) {
        if no_interactive {
            bail!(
                "No .vscode directory found in current directory.\nFound .vscode in parent: {}\nIn non-interactive mode, cannot change directory.\nPlease cd there and run the command again.",
                parent_path.display()
            );
        }
        let use_parent = prompt_yes_no(
            &format!(
                "Found a .vscode project in {}. Install there?",
                parent_path.display()
            ),
            true,
        )?;
        if use_parent {
            std::env::set_current_dir(&parent_path)
                .with_context(|| format!("cd {}", parent_path.display()))?;
            return Ok(());
        }
    }

    if detect_supported_ide().is_none() {
        bail!("No supported IDE found (VSCode or Cursor). Please install VSCode or Cursor first.");
    }

    if no_interactive {
        bail!(
            "No .vscode directory found in current directory or parent directories.\nIn non-interactive mode, cannot create a new project.\nPlease create a .vscode directory or run without --no-interactive."
        );
    }

    println!("No .vscode directory found in current directory or parent directories.");
    if prompt_yes_no(
        "Would you like to generate a VSCode project with FastLED configuration?",
        true,
    )? {
        generate_vscode_project()?;
        return Ok(());
    }

    bail!("installation cancelled");
}

fn detect_fastled_project() -> bool {
    let Ok(cwd) = std::env::current_dir() else {
        return false;
    };
    let library_json = cwd.join("library.json");
    let Ok(text) = fs::read_to_string(library_json) else {
        return false;
    };
    serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|value| value.get("name").and_then(Value::as_str).map(str::to_owned))
        .map(|name| name == "FastLED")
        .unwrap_or(false)
}

fn is_fastled_repository() -> bool {
    let Ok(cwd) = std::env::current_dir() else {
        return false;
    };
    let required_markers = [
        cwd.join("src").join("FastLED.h"),
        cwd.join("examples").join("Blink").join("Blink.ino"),
        cwd.join("ci").join("ci-compile.py"),
        cwd.join("src").join("platforms"),
        cwd.join("library.json"),
    ];
    if required_markers.iter().any(|path| !path.exists()) {
        return false;
    }

    let Ok(text) = fs::read_to_string(cwd.join("library.json")) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return false;
    };
    if value.get("name").and_then(Value::as_str) != Some("FastLED") {
        return false;
    }
    if !value
        .get("repository")
        .and_then(|repo| repo.get("url"))
        .and_then(Value::as_str)
        .map(|url| url.contains("FastLED/FastLED"))
        .unwrap_or(false)
    {
        return false;
    }

    let tests_dir = cwd.join("tests");
    if !tests_dir.is_dir() {
        return false;
    }
    fs::read_dir(tests_dir)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.flatten())
        .any(|entry| {
            entry.path().is_file()
                && entry.file_name().to_string_lossy().starts_with("test_")
                && entry.path().extension().and_then(|ext| ext.to_str()) == Some("cpp")
        })
}

fn check_existing_arduino_content() -> bool {
    fn has_ino_file(dir: &Path) -> bool {
        let Ok(entries) = fs::read_dir(dir) else {
            return false;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if has_ino_file(&path) {
                    return true;
                }
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) == Some("ino") {
                return true;
            }
        }
        false
    }

    let Ok(cwd) = std::env::current_dir() else {
        return false;
    };
    cwd.join("examples").exists() || has_ino_file(&cwd)
}

fn read_json_file(path: &Path, default: Value) -> Value {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .unwrap_or(default)
}

fn write_json_file(path: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let mut text = serde_json::to_string_pretty(value).context("serialize JSON")?;
    text.push('\n');
    fs::write(path, text).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn update_launch_json_for_arduino() -> Result<()> {
    let cwd = std::env::current_dir().context("current dir")?;
    let launch_json_path = cwd.join(".vscode").join("launch.json");
    let mut data = read_json_file(
        &launch_json_path,
        json!({"version": "0.2.0", "configurations": []}),
    );

    if !data.is_object() {
        data = json!({"version": "0.2.0", "configurations": []});
    }

    let arduino_config = json!({
        "name": "Auto Debug (Smart File Detection)",
        "type": "auto-debug",
        "request": "launch",
        "map": {
            "*.ino": "Arduino: Run .ino with FastLED",
            "*.py": "Python: Current File (UV)"
        }
    });

    let configs = data
        .as_object_mut()
        .expect("launch.json root object")
        .entry("configurations")
        .or_insert_with(|| Value::Array(Vec::new()));
    if !configs.is_array() {
        *configs = Value::Array(Vec::new());
    }
    let configs_array = configs.as_array_mut().expect("configurations array");
    let exists = configs_array.iter().any(|cfg| {
        cfg.get("name").and_then(Value::as_str)
            == arduino_config.get("name").and_then(Value::as_str)
    });
    if !exists {
        configs_array.insert(0, arduino_config);
    }

    write_json_file(&launch_json_path, &data)?;
    println!("Updated {}", launch_json_path.display());
    Ok(())
}

fn generate_fastled_tasks() -> Result<()> {
    let cwd = std::env::current_dir().context("current dir")?;
    let tasks_json_path = cwd.join(".vscode").join("tasks.json");
    let mut data = read_json_file(&tasks_json_path, json!({"version": "2.0.0", "tasks": []}));

    if !data.is_object() {
        data = json!({"version": "2.0.0", "tasks": []});
    }

    let fastled_tasks = vec![
        json!({
            "type": "shell",
            "label": "Run FastLED (Debug)",
            "command": "fastled",
            "args": ["${file}", "--debug"],
            "options": {"cwd": "${workspaceFolder}"},
            "group": {"kind": "build", "isDefault": true},
            "presentation": {
                "echo": true,
                "reveal": "always",
                "focus": true,
                "panel": "new",
                "showReuseMessage": false,
                "clear": true
            },
            "detail": "Run FastLED with debug mode and Tauri visualization",
            "problemMatcher": []
        }),
        json!({
            "type": "shell",
            "label": "Run FastLED (Quick)",
            "command": "fastled",
            "args": ["${file}", "--quick"],
            "options": {"cwd": "${workspaceFolder}"},
            "group": "build",
            "presentation": {
                "echo": true,
                "reveal": "always",
                "focus": true,
                "panel": "new",
                "showReuseMessage": false,
                "clear": true
            },
            "detail": "Run FastLED with quick build mode",
            "problemMatcher": []
        }),
    ];

    let tasks = data
        .as_object_mut()
        .expect("tasks.json root object")
        .entry("tasks")
        .or_insert_with(|| Value::Array(Vec::new()));
    if !tasks.is_array() {
        *tasks = Value::Array(Vec::new());
    }
    let tasks_array = tasks.as_array_mut().expect("tasks array");
    let existing_labels: Vec<String> = tasks_array
        .iter()
        .filter_map(|task| task.get("label").and_then(Value::as_str).map(str::to_owned))
        .collect();

    for task in fastled_tasks {
        let Some(label) = task.get("label").and_then(Value::as_str) else {
            continue;
        };
        if !existing_labels.iter().any(|existing| existing == label) {
            tasks_array.push(task);
        }
    }

    write_json_file(&tasks_json_path, &data)?;
    println!("Updated {}", tasks_json_path.display());
    Ok(())
}

fn fastled_repository_settings() -> Value {
    json!({
        "terminal.integrated.defaultProfile.windows": "Git Bash",
        "terminal.integrated.shellIntegration.enabled": false,
        "terminal.integrated.profiles.windows": {
            "Command Prompt": {"path": "C:\\Windows\\System32\\cmd.exe"},
            "Git Bash": {
                "path": "C:\\Program Files\\Git\\bin\\bash.exe",
                "args": ["--cd=."]
            }
        },
        "files.eol": "\n",
        "files.autoDetectEol": false,
        "files.insertFinalNewline": true,
        "files.trimFinalNewlines": true,
        "editor.tabSize": 4,
        "editor.insertSpaces": true,
        "editor.detectIndentation": true,
        "editor.formatOnSave": false,
        "debug.defaultDebuggerType": "cppdbg",
        "debug.toolBarLocation": "docked",
        "debug.console.fontSize": 14,
        "debug.console.lineHeight": 19,
        "python.defaultInterpreterPath": "uv",
        "python.debugger": "debugpy",
        "[cpp]": {
            "editor.defaultFormatter": "llvm-vs-code-extensions.vscode-clangd",
            "debug.defaultDebuggerType": "cppdbg"
        },
        "[c]": {
            "editor.defaultFormatter": "ms-vscode.cpptools",
            "debug.defaultDebuggerType": "cppdbg"
        },
        "[ino]": {
            "editor.defaultFormatter": "ms-vscode.cpptools",
            "debug.defaultDebuggerType": "cppdbg"
        },
        "clangd.arguments": [
            "--compile-commands-dir=${workspaceFolder}",
            "--clang-tidy",
            "--header-insertion=never",
            "--completion-style=detailed",
            "--function-arg-placeholders=false",
            "--background-index",
            "--pch-storage=memory"
        ],
        "clangd.fallbackFlags": [
            "-std=c++17",
            "-I${workspaceFolder}/src",
            "-I${workspaceFolder}/tests",
            "-Wno-global-constructors"
        ],
        "C_Cpp.intelliSenseEngine": "disabled",
        "C_Cpp.autocomplete": "disabled",
        "C_Cpp.errorSquiggles": "disabled",
        "C_Cpp.suggestSnippets": false,
        "C_Cpp.intelliSenseEngineFallback": "disabled",
        "C_Cpp.autocompleteAddParentheses": false,
        "C_Cpp.formatting": "disabled",
        "C_Cpp.vcpkg.enabled": false,
        "C_Cpp.configurationWarnings": "disabled",
        "C_Cpp.intelliSenseCachePath": "",
        "C_Cpp.intelliSenseCacheSize": 0,
        "C_Cpp.intelliSenseUpdateDelay": 0,
        "C_Cpp.workspaceParsingPriority": "lowest",
        "C_Cpp.disabled": true,
        "files.associations": {
            "*.ino": "cpp",
            "*.h": "cpp",
            "*.hpp": "cpp",
            "*.cpp": "cpp",
            "*.c": "c",
            "*.inc": "cpp",
            "*.tcc": "cpp",
            "*.embeddedhtml": "html",
            "compare": "cpp",
            "type_traits": "cpp",
            "cmath": "cpp",
            "limits": "cpp",
            "iostream": "cpp",
            "random": "cpp",
            "functional": "cpp",
            "bit": "cpp",
            "vector": "cpp",
            "array": "cpp",
            "string": "cpp",
            "memory": "cpp",
            "algorithm": "cpp",
            "iterator": "cpp",
            "utility": "cpp",
            "optional": "cpp",
            "variant": "cpp",
            "numeric": "cpp",
            "chrono": "cpp",
            "thread": "cpp",
            "mutex": "cpp",
            "atomic": "cpp",
            "future": "cpp",
            "condition_variable": "cpp"
        },
        "java.enabled": false,
        "java.jdt.ls.enabled": false,
        "java.compile.nullAnalysis.mode": "disabled",
        "java.configuration.checkProjectSettingsExclusions": false,
        "java.import.gradle.enabled": false,
        "java.import.maven.enabled": false,
        "java.autobuild.enabled": false,
        "java.maxConcurrentBuilds": 0,
        "java.recommendations.enabled": false,
        "java.help.showReleaseNotes": false,
        "redhat.telemetry.enabled": false,
        "java.project.sourcePaths": [],
        "java.project.referencedLibraries": [],
        "files.exclude": {
            "**/.classpath": true,
            "**/.project": true,
            "**/.factorypath": true
        },
        "platformio.disableToolchainAutoInstaller": true,
        "platformio-ide.autoRebuildAutocompleteIndex": false,
        "platformio-ide.activateProjectOnTextEditorChange": false,
        "platformio-ide.autoOpenPlatformIOIniFile": false,
        "platformio-ide.autoPreloadEnvTasks": false,
        "platformio-ide.autoCloseSerialMonitor": false,
        "platformio-ide.disablePIOHomeStartup": true,
        "extensions.ignoreRecommendations": true,
        "editor.semanticTokenColorCustomizations": {
            "rules": {
                "class": "#4EC9B0",
                "struct": "#4EC9B0",
                "type": "#4EC9B0",
                "enum": "#4EC9B0",
                "enumMember": "#B5CEA8",
                "typedef": "#4EC9B0",
                "variable": "#FAFAFA",
                "variable.local": "#FAFAFA",
                "parameter": "#FF8C42",
                "variable.parameter": "#FF8C42",
                "property": "#D197D9",
                "function": "#DCDCAA",
                "method": "#DCDCAA",
                "function.declaration": "#DCDCAA",
                "method.declaration": "#DCDCAA",
                "namespace": "#86C5F7",
                "variable.readonly": {"foreground": "#B5CEA8", "fontStyle": "italic"},
                "variable.defaultLibrary": "#B5CEA8",
                "macro": "#E06C75",
                "string": "#CE9178",
                "number": "#B5CEA8",
                "keyword": "#C586C0",
                "keyword.storage": "#FF79C6",
                "storageClass": "#FF79C6",
                "type.builtin": "#569CD6",
                "keyword.type": "#569CD6",
                "comment": "#6A9955",
                "comment.documentation": "#6A9955"
            }
        },
        "editor.inlayHints.fontColor": "#808080",
        "editor.inlayHints.background": "#3C3C3C20"
    })
}

fn update_vscode_settings_for_fastled() -> Result<()> {
    if !is_fastled_repository() {
        return Ok(());
    }

    let cwd = std::env::current_dir().context("current dir")?;
    let settings_json_path = cwd.join(".vscode").join("settings.json");
    let mut data = read_json_file(&settings_json_path, json!({}));
    if !data.is_object() {
        data = json!({});
    }

    let settings = fastled_repository_settings();
    let target = data.as_object_mut().expect("settings root object");
    let source = settings.as_object().expect("settings object");
    for (key, value) in source {
        target.insert(key.clone(), value.clone());
    }

    write_json_file(&settings_json_path, &data)?;
    println!(
        "Updated {} with comprehensive FastLED development settings",
        settings_json_path.display()
    );
    Ok(())
}

fn download_to_path(url: &str, dest: &Path) -> Result<()> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .context("build HTTP client")?;
    let bytes = client
        .get(url)
        .send()
        .with_context(|| format!("GET {url} failed"))?
        .error_for_status()
        .with_context(|| format!("download returned error for {url}"))?
        .bytes()
        .context("read response bytes")?;
    fs::write(dest, &bytes).with_context(|| format!("write {}", dest.display()))?;
    Ok(())
}

fn install_auto_debug_extension(dry_run: bool) -> Result<bool> {
    if dry_run {
        println!("[DRY-RUN]: Would download and install Auto Debug extension");
        return Ok(true);
    }

    let Some((ide_command, ide_name)) = detect_supported_ide() else {
        println!("Warning: no supported IDE found (VSCode or Cursor)");
        return Ok(false);
    };

    let temp_dir = tempfile::tempdir().context("create temp dir for extension")?;
    let vsix_path = temp_dir.path().join("auto-debug.vsix");
    println!("Downloading Auto Debug extension...");
    download_to_path(AUTO_DEBUG_VSIX_URL, &vsix_path)?;
    println!("Installing extension in {ide_name}...");

    let status = Command::new(ide_command)
        .args(["--install-extension", &vsix_path.to_string_lossy()])
        .status()
        .with_context(|| format!("launch {ide_command} for extension install"))?;

    if !status.success() {
        println!("Warning: extension installer exited with {}", status);
        return Ok(false);
    }

    println!("Auto Debug extension installed in {ide_name}");
    Ok(true)
}

fn install_default_example() -> Result<bool> {
    let output_dir = PathBuf::from("fastled");
    let repo_root = ensure_fastled_repo(None)?;
    let resolved_ref = crate::project::cached_repo_ref_name(&repo_root);
    let out = crate::project::init_example_from_repo(
        &repo_root,
        DEFAULT_INSTALL_EXAMPLE,
        &output_dir,
        Some(resolved_ref.as_str()),
    )?;
    println!("Installed example at {}", out.display());
    Ok(true)
}

pub fn run_install(options: InstallOptions) -> Result<InstallOutcome> {
    println!("Starting FastLED installation...");
    validate_vscode_project(options.no_interactive)?;

    let is_fastled_project = detect_fastled_project();
    let is_repository = is_fastled_repository();
    if is_fastled_project {
        if is_repository {
            println!("Detected FastLED repository - configuring full development environment");
        } else {
            println!("Detected external FastLED project - configuring Arduino environment");
        }
    } else {
        println!("Detected standard project - configuring basic Arduino environment");
    }

    let should_install_extension = if options.no_interactive {
        println!("Skipping Auto Debug extension installation in non-interactive mode");
        false
    } else if options.dry_run {
        println!("[DRY-RUN]: Simulating Auto Debug extension installation...");
        true
    } else {
        prompt_yes_no(
            "Would you like to install the FastLED auto-debug extension?",
            false,
        )?
    };

    if should_install_extension && !install_auto_debug_extension(options.dry_run)? {
        println!("Warning: Auto Debug extension installation failed, continuing...");
    }

    println!("\nConfiguring VSCode files...");
    update_launch_json_for_arduino()?;
    generate_fastled_tasks()?;

    let mut launch_after = false;
    if !check_existing_arduino_content() {
        if options.no_interactive {
            println!(
                "No Arduino content found. In non-interactive mode, skipping example installation."
            );
        } else if prompt_yes_no(
            &format!(
                "No Arduino content found. Install the default '{}' example?",
                DEFAULT_INSTALL_EXAMPLE
            ),
            true,
        )? {
            if options.dry_run {
                println!(
                    "[DRY-RUN]: Would initialize the default '{}' example",
                    DEFAULT_INSTALL_EXAMPLE
                );
            } else {
                launch_after = install_default_example()?;
            }
        }
    } else {
        println!("Existing Arduino content detected, skipping example installation");
        launch_after = !options.dry_run;
    }

    if is_fastled_project {
        if is_repository {
            println!("\nSetting up FastLED development environment...");
            update_vscode_settings_for_fastled()?;
        } else {
            println!("\nSkipping clangd settings - not in the FastLED repository");
        }
    }

    if options.dry_run {
        println!("\n[DRY-RUN]: Skipping auto-launch");
        launch_after = false;
    }

    println!("\nFastLED installation completed successfully!");
    Ok(InstallOutcome { launch_after })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DARWIN_ARM64_MANIFEST: &str = r#"{
  "latest": "releases-d70a5da89b3e673bf6a482724478fc17e81e575e",
  "versions": {
    "releases-d70a5da89b3e673bf6a482724478fc17e81e575e": {
      "version": "releases-d70a5da89b3e673bf6a482724478fc17e81e575e",
      "href": "https://media.githubusercontent.com/media/zackees/clang-tool-chain-bins/main/assets/emscripten/darwin/arm64/emscripten-releases-d70a5da89b3e673bf6a482724478fc17e81e575e-darwin-arm64.tar.zst",
      "sha256": "6af749f0d44927c7d4c93c9e407e195257ed690b8e3bd3a43d7a3f4badc52082"
    }
  }
}"#;

    const LINUX_X86_64_MANIFEST_WITH_PARTS: &str = r#"{
  "latest": "4.0.21",
  "versions": {
    "4.0.21": {
      "version": "4.0.21",
      "href": "https://raw.githubusercontent.com/zackees/clang-tool-chain-bins/main/assets/emscripten/linux/x86_64/emscripten-4.0.21-linux-x86_64.tar.zst",
      "sha256": "5cd3cbe0316d37c9b39bdc63691c014f136a5d82a9f08ed29bb7ad62f7a83655",
      "parts": [
        {
          "href": "https://raw.githubusercontent.com/zackees/clang-tool-chain-bins/main/assets/emscripten/linux/x86_64/emscripten-4.0.21-linux-x86_64.tar.zst.part-aa",
          "sha256": "e427aee7d1197f59bcbd7a82a581f8d0bdf484ea24036a3c52903bb26cfd4488",
          "size": 99614720
        }
      ]
    }
  }
}"#;

    #[test]
    fn parses_nested_versions_manifest_without_parts() {
        let manifest: PlatformManifest =
            serde_json::from_str(DARWIN_ARM64_MANIFEST).expect("parse darwin/arm64 manifest");
        assert_eq!(
            manifest.latest,
            "releases-d70a5da89b3e673bf6a482724478fc17e81e575e"
        );
        let entry = manifest
            .versions
            .get(&manifest.latest)
            .expect("entry for latest");
        assert!(entry.href.contains("emscripten-releases-"));
        assert_eq!(
            entry.sha256,
            "6af749f0d44927c7d4c93c9e407e195257ed690b8e3bd3a43d7a3f4badc52082"
        );
        assert!(entry.parts.is_none());
    }

    #[test]
    fn parses_manifest_with_multipart_archive_and_extra_size_field() {
        let manifest: PlatformManifest = serde_json::from_str(LINUX_X86_64_MANIFEST_WITH_PARTS)
            .expect("parse linux/x86_64 manifest");
        let entry = manifest.versions.get("4.0.21").expect("entry for 4.0.21");
        let parts = entry.parts.as_ref().expect("parts present");
        assert_eq!(parts.len(), 1);
        assert!(parts[0].href.ends_with(".part-aa"));
    }

    /// Regression for issue #111: the win/x86_64 manifest publishes version
    /// entries as siblings of `latest` rather than under a `versions` map.
    /// The CLI must accept that legacy shape.
    #[test]
    fn parses_legacy_flat_manifest_shape() {
        let text = r#"{
            "latest": "4.0.19",
            "4.0.19": {
                "href": "https://example.com/em-4.0.19.tar.zst",
                "sha256": "b19c2e35b863eb17866034f917d7957514645e179e9d22800729b0dcbb2aa2e2"
            }
        }"#;
        let manifest = parse_platform_manifest(text).expect("parse legacy manifest");
        assert_eq!(manifest.latest, "4.0.19");
        let entry = manifest
            .versions
            .get("4.0.19")
            .expect("entry for latest version");
        assert_eq!(entry.sha256.len(), 64);
        assert!(entry.href.ends_with(".tar.zst"));
    }

    #[test]
    fn legacy_manifest_with_no_versions_errors() {
        let text = r#"{ "latest": "4.0.19" }"#;
        assert!(parse_platform_manifest(text).is_err());
    }

    #[test]
    fn legacy_manifest_with_unknown_latest_errors() {
        let text = r#"{
            "latest": "9.9.9",
            "4.0.19": {
                "href": "https://example.com/em.tar.zst",
                "sha256": "abcdef"
            }
        }"#;
        assert!(parse_platform_manifest(text).is_err());
    }
}
