//! Toolchain install entry points driven from the Rust CLI.
//!
//! Mirrors `src/fastled/toolchain/emscripten_archive.py` so the Python side
//! no longer needs `httpx` / `pyzstd` to materialise the emscripten toolchain.
//! Public entry points are intended to be called once at the top of the
//! compile flow; results are cached on disk via a `done.txt` marker.

use std::fs;
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use kernal_api::json::{self, Layout, Value as JsonValue};

use crate::archive;

/// Toolchain platform manifest as published in
/// `clang-tool-chain-bins/assets/emscripten/{platform}/{arch}/manifest.json`.
///
/// Local mirror of the schema rather than relying on `ctcb-manifest` because
/// the published manifests use a `"versions": { … }` map, while older
/// `ctcb-manifest` releases expected version keys at the top level. Keeping
/// the schema local insulates the CLI from upstream crate drift.
#[cfg(test)]
#[derive(Debug, Clone)]
struct PlatformManifest {
    latest: String,
    versions: BTreeMap<String, VersionInfo>,
}

#[cfg(test)]
#[derive(Debug, Clone)]
struct VersionInfo {
    href: String,
    sha256: String,
    parts: Option<Vec<PartRef>>,
}

#[cfg(test)]
#[derive(Debug, Clone)]
struct PartRef {
    href: String,
    sha256: String,
}

const ESBUILD_VERSION: &str = "0.28.0";
const EMSCRIPTEN_VERSION_MARKER: &str = ".fastled-manifest-version";

static EMSCRIPTEN_INSTALL_CACHE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

const FASTLED_REPO: &str = "FastLED/FastLED";
const FASTLED_LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/FastLED/FastLED/releases/latest";

fn fastled_root() -> Result<PathBuf> {
    let home =
        kernal_api::platform::fs::user_home_dir().context("cannot resolve home directory")?;
    Ok(home.join(".fastled"))
}

fn detect_platform_arch() -> Result<(String, String)> {
    let target = kernal_api::platform::host::process_target();
    toolchain_platform_arch(target.os, target.architecture)
        .context("detect host clang-tool-chain target")
        .map(|(platform, arch)| (platform.to_owned(), arch.to_owned()))
}

/// Clang-tool-chain catalog names and supported targets are product policy.
fn toolchain_platform_arch(os: &str, architecture: &str) -> Result<(&'static str, &'static str)> {
    let platform = match os {
        "windows" => "win",
        "linux" => "linux",
        "macos" => "darwin",
        _ => bail!("unsupported operating system"),
    };
    let arch = match architecture {
        "x86_64" => "x86_64",
        "aarch64" => "arm64",
        other => bail!("unsupported architecture: {other}"),
    };
    Ok((platform, arch))
}

/// Parse a platform manifest, accepting both the current schema (`versions`
/// sub-map) and the historical layout where version keys live at the top
/// level next to `latest`. The two formats coexist on the assets server
/// today, so the CLI has to handle both.
#[cfg(test)]
fn parse_platform_manifest(text: &str) -> Result<PlatformManifest> {
    fn version(value: JsonValue) -> Result<VersionInfo> {
        let value = match value {
            JsonValue::Array(mut fields) if fields.len() == 2 => {
                fields.push(JsonValue::Null);
                JsonValue::Array(fields)
            }
            other => other,
        };
        let [href, sha256, parts] = toolchain_record_value(value, ["href", "sha256", "parts"])?;
        let parts = match parts {
            None | Some(JsonValue::Null) => None,
            Some(JsonValue::Array(values)) => Some(
                values
                    .into_iter()
                    .map(|value| {
                        let [href, sha256] = toolchain_record_value(value, ["href", "sha256"])?;
                        Ok(PartRef {
                            href: toolchain_string(href)?,
                            sha256: toolchain_string(sha256)?,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?,
            ),
            _ => bail!("invalid archive parts"),
        };
        Ok(VersionInfo {
            href: toolchain_string(href)?,
            sha256: toolchain_string(sha256)?,
            parts,
        })
    }
    fn nested(text: &str) -> Result<PlatformManifest> {
        let [latest, versions] = toolchain_record(text.as_bytes(), ["latest", "versions"])?;
        let Some(JsonValue::ObjectMembers(members)) = versions else {
            bail!("missing versions");
        };
        let mut versions = BTreeMap::new();
        for (key, value) in members {
            versions.insert(key, version(value)?);
        }
        Ok(PlatformManifest {
            latest: toolchain_string(latest)?,
            versions,
        })
    }
    if let Ok(parsed) = nested(text) {
        return Ok(parsed);
    }

    let JsonValue::Object(mut object) = json::parse(text.as_bytes())? else {
        bail!("manifest is not a JSON object");
    };
    let latest = toolchain_string(object.remove("latest"))?;

    let mut versions = BTreeMap::new();
    for (key, value) in object {
        let info = version(value).with_context(|| format!("parse version entry `{key}`"))?;
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

#[cfg(test)]
fn multipart_parts(version_info: &VersionInfo) -> Option<&[PartRef]> {
    version_info
        .parts
        .as_deref()
        .filter(|parts| !parts.is_empty())
}

#[cfg(unix)]
fn ensure_toolchain_executables(install_dir: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fn add_execute_bits(path: &Path) -> Result<()> {
        let metadata =
            fs::metadata(path).with_context(|| format!("read metadata for {}", path.display()))?;
        if !metadata.is_file() {
            return Ok(());
        }
        let mut permissions = metadata.permissions();
        let mode = permissions.mode();
        if mode & 0o111 == 0 {
            permissions.set_mode(mode | 0o111);
            fs::set_permissions(path, permissions)
                .with_context(|| format!("set executable bit on {}", path.display()))?;
        }
        Ok(())
    }

    let bin_dir = install_dir.join("bin");
    let mut pending = if bin_dir.is_dir() {
        vec![bin_dir]
    } else {
        Vec::new()
    };
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).with_context(|| format!("read {}", dir.display()))? {
            let entry = entry.with_context(|| format!("read entry in {}", dir.display()))?;
            let path = entry.path();
            let metadata = fs::metadata(&path)
                .with_context(|| format!("read metadata for {}", path.display()))?;
            if metadata.is_dir() {
                pending.push(path);
                continue;
            }
            if !metadata.is_file() {
                continue;
            }

            add_execute_bits(&path)?;
        }
    }

    // Emscripten invokes these extensionless launchers directly while
    // building system libraries. They live outside bin/, and some published
    // archives lose their Unix mode bits.
    for name in [
        "em++",
        "emar",
        "embuilder",
        "emcc",
        "emcmake",
        "emconfigure",
        "emmake",
        "emnm",
        "emranlib",
        "emrun",
        "emsize",
        "emstrip",
    ] {
        let launcher = install_dir.join("emscripten").join(name);
        if launcher.exists() {
            add_execute_bits(&launcher)?;
        }
    }

    Ok(())
}

#[cfg(not(unix))]
fn ensure_toolchain_executables(_install_dir: &Path) -> Result<()> {
    Ok(())
}

fn required_emscripten_payload_files(install_dir: &Path) -> Vec<PathBuf> {
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    vec![
        install_dir.join("emscripten/emcc.py"),
        install_dir.join("emscripten/em++.py"),
        install_dir.join("emscripten/emar.py"),
        install_dir.join("emscripten/emscripten-version.txt"),
        install_dir.join(format!("bin/clang++{suffix}")),
        install_dir.join(format!("bin/wasm-ld{suffix}")),
        install_dir.join(format!("bin/llvm-ar{suffix}")),
        install_dir.join(format!("bin/llvm-objcopy{suffix}")),
        install_dir.join(format!("bin/wasm-emscripten-finalize{suffix}")),
    ]
}

fn validate_emscripten_payload(install_dir: &Path) -> Result<()> {
    let missing = required_emscripten_payload_files(install_dir)
        .into_iter()
        .filter(|path| {
            !fs::metadata(path)
                .map(|metadata| metadata.is_file() && metadata.len() > 0)
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(());
    }
    bail!(
        "Emscripten installation is incomplete; missing or empty files: {}",
        missing
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn validate_complete_emscripten_install(install_dir: &Path) -> Result<()> {
    validate_emscripten_payload(install_dir)?;
    let done = install_dir.join("done.txt");
    if !done.is_file()
        || fs::metadata(&done)
            .map(|metadata| metadata.len())
            .unwrap_or(0)
            == 0
    {
        bail!(
            "Emscripten installation is missing a complete marker: {}",
            done.display()
        );
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolchainSpec {
    pub platform: &'static str,
    pub arch: &'static str,
    pub package_id: &'static str,
    pub archive_url: &'static str,
    pub archive_sha256: &'static str,
    pub archive_parts: &'static [ToolchainPart],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolchainPart {
    pub url: &'static str,
    pub sha256: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ToolchainReceipt {
    schema_version: u32,
    catalog_commit: String,
    platform: String,
    arch: String,
    package_id: String,
    archive_url: String,
    archive_sha256: String,
    health_checked: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ActiveToolchainState {
    schema_version: u32,
    active: Option<String>,
    previous_known_good: Option<String>,
}

// Record schemas are installer policy. The kernel owns JSON syntax, duplicate
// preservation and resource limits; unknown fields remain forward-compatible.
fn toolchain_record<const N: usize>(
    bytes: &[u8],
    names: [&str; N],
) -> Result<[Option<JsonValue>; N]> {
    toolchain_record_value(json::parse_members(bytes)?, names)
}

fn toolchain_record_value<const N: usize>(
    value: JsonValue,
    names: [&str; N],
) -> Result<[Option<JsonValue>; N]> {
    let mut fields = std::array::from_fn(|_| None);
    let value = match value {
        JsonValue::Object(value) => JsonValue::ObjectMembers(value.into_iter().collect()),
        other => other,
    };
    match value {
        JsonValue::ObjectMembers(members) => {
            for (name, value) in members {
                let Some(index) = names.iter().position(|known| *known == name) else {
                    continue;
                };
                if fields[index].replace(value).is_some() {
                    bail!("duplicate toolchain record field: {}", names[index]);
                }
            }
        }
        JsonValue::Array(values) if values.len() == N => {
            for (field, value) in fields.iter_mut().zip(values) {
                *field = Some(value);
            }
        }
        _ => bail!("invalid toolchain record"),
    }
    Ok(fields)
}

fn toolchain_schema(value: Option<JsonValue>) -> Result<u32> {
    match value {
        Some(JsonValue::Signed(value)) => Ok(u32::try_from(value)?),
        Some(JsonValue::Unsigned(value)) => Ok(u32::try_from(value)?),
        _ => bail!("missing or invalid toolchain schema version"),
    }
}

fn toolchain_string(value: Option<JsonValue>) -> Result<String> {
    match value {
        Some(JsonValue::String(value)) => Ok(value),
        _ => bail!("missing or invalid toolchain string field"),
    }
}

impl ActiveToolchainState {
    fn parse(bytes: &[u8]) -> Result<Self> {
        let [schema, active, previous] =
            toolchain_record(bytes, ["schema_version", "active", "previous_known_good"])?;
        let optional_string = |value| match value {
            None | Some(JsonValue::Null) => Ok(None),
            other => toolchain_string(other).map(Some),
        };
        Ok(Self {
            schema_version: toolchain_schema(schema)?,
            active: optional_string(active)?,
            previous_known_good: optional_string(previous)?,
        })
    }

    fn encode(&self) -> Result<Vec<u8>> {
        Ok(json::encode(
            &JsonValue::ObjectMembers(vec![
                (
                    "schema_version".into(),
                    JsonValue::Unsigned(self.schema_version.into()),
                ),
                (
                    "active".into(),
                    self.active
                        .clone()
                        .map_or(JsonValue::Null, JsonValue::String),
                ),
                (
                    "previous_known_good".into(),
                    self.previous_known_good
                        .clone()
                        .map_or(JsonValue::Null, JsonValue::String),
                ),
            ]),
            Layout::Pretty,
        )?)
    }
}

impl ToolchainReceipt {
    fn parse(bytes: &[u8]) -> Result<Self> {
        let [schema, catalog, platform, arch, package, url, hash, health] = toolchain_record(
            bytes,
            [
                "schema_version",
                "catalog_commit",
                "platform",
                "arch",
                "package_id",
                "archive_url",
                "archive_sha256",
                "health_checked",
            ],
        )?;
        let Some(JsonValue::Bool(health_checked)) = health else {
            bail!("missing or invalid toolchain health flag");
        };
        Ok(Self {
            schema_version: toolchain_schema(schema)?,
            catalog_commit: match catalog {
                None => String::new(),
                other => toolchain_string(other)?,
            },
            platform: toolchain_string(platform)?,
            arch: toolchain_string(arch)?,
            package_id: toolchain_string(package)?,
            archive_url: toolchain_string(url)?,
            archive_sha256: toolchain_string(hash)?,
            health_checked,
        })
    }

    fn encode(&self) -> Result<Vec<u8>> {
        Ok(json::encode(
            &JsonValue::ObjectMembers(vec![
                (
                    "schema_version".into(),
                    JsonValue::Unsigned(self.schema_version.into()),
                ),
                (
                    "catalog_commit".into(),
                    JsonValue::String(self.catalog_commit.clone()),
                ),
                ("platform".into(), JsonValue::String(self.platform.clone())),
                ("arch".into(), JsonValue::String(self.arch.clone())),
                (
                    "package_id".into(),
                    JsonValue::String(self.package_id.clone()),
                ),
                (
                    "archive_url".into(),
                    JsonValue::String(self.archive_url.clone()),
                ),
                (
                    "archive_sha256".into(),
                    JsonValue::String(self.archive_sha256.clone()),
                ),
                (
                    "health_checked".into(),
                    JsonValue::Bool(self.health_checked),
                ),
            ]),
            Layout::Pretty,
        )?)
    }
}

const TOOLCHAIN_STATE_SCHEMA: u32 = 1;
const TOOLCHAIN_RECEIPT_SCHEMA: u32 = 1;
const TOOLCHAIN_CATALOG_COMMIT: &str = "ef4a0e4a767c46528776105815033fb870ec337a";
const TOOLCHAIN_STATE_FILE: &str = ".fastled-active-toolchain.json";
const TOOLCHAIN_RECEIPT_FILE: &str = ".fastled-toolchain.json";

pub fn release_default_toolchain() -> Result<ToolchainSpec> {
    let (platform, arch) = detect_platform_arch()?;
    catalog_toolchain(&platform, &arch)
}

fn catalog_toolchain(platform: &str, arch: &str) -> Result<ToolchainSpec> {
    let commit = TOOLCHAIN_CATALOG_COMMIT;
    match (platform, arch) {
        ("win", "x86_64") => Ok(ToolchainSpec {
            platform: "win",
            arch: "x86_64",
            package_id: "4.0.19",
            archive_url: "https://media.githubusercontent.com/media/zackees/clang-tool-chain-bins/ef4a0e4a767c46528776105815033fb870ec337a/assets/emscripten/win/x86_64/emscripten-latest-win-x86_64.tar.zst",
            archive_sha256: "b19c2e35b863eb17866034f917d7957514645e179e9d22800729b0dcbb2aa2e2",
            archive_parts: &[],
        }),
        ("linux", "x86_64") => Ok(ToolchainSpec {
            platform: "linux",
            arch: "x86_64",
            package_id: "4.0.21",
            archive_url: "https://media.githubusercontent.com/media/zackees/clang-tool-chain-bins/ef4a0e4a767c46528776105815033fb870ec337a/assets/emscripten/linux/x86_64/emscripten-4.0.21-linux-x86_64.tar.zst",
            archive_sha256: "5cd3cbe0316d37c9b39bdc63691c014f136a5d82a9f08ed29bb7ad62f7a83655",
            archive_parts: &[],
        }),
        ("linux", "arm64") => Ok(ToolchainSpec {
            platform: "linux",
            arch: "arm64",
            package_id: "4.0.21",
            archive_url: "https://media.githubusercontent.com/media/zackees/clang-tool-chain-bins/ef4a0e4a767c46528776105815033fb870ec337a/assets/emscripten/linux/arm64/emscripten-4.0.21-linux-arm64.tar.zst",
            archive_sha256: "610375cc8e88fabe47a1675e747e8aade31279eb1e6ec2bad6a355e6376af16f",
            archive_parts: &[],
        }),
        ("darwin", "x86_64") => Ok(ToolchainSpec {
            platform: "darwin",
            arch: "x86_64",
            package_id: "releases-d70a5da89b3e673bf6a482724478fc17e81e575e",
            archive_url: "https://media.githubusercontent.com/media/zackees/clang-tool-chain-bins/ef4a0e4a767c46528776105815033fb870ec337a/assets/emscripten/darwin/x86_64/emscripten-releases-d70a5da89b3e673bf6a482724478fc17e81e575e-darwin-x86_64.tar.zst",
            archive_sha256: "6ba74e00642568383798a7ccd3b643ce3c5cd5606789bbe824aa9971f0d8894f",
            archive_parts: &[],
        }),
        ("darwin", "arm64") => Ok(ToolchainSpec {
            platform: "darwin",
            arch: "arm64",
            package_id: "releases-d70a5da89b3e673bf6a482724478fc17e81e575e",
            archive_url: "https://media.githubusercontent.com/media/zackees/clang-tool-chain-bins/ef4a0e4a767c46528776105815033fb870ec337a/assets/emscripten/darwin/arm64/emscripten-releases-d70a5da89b3e673bf6a482724478fc17e81e575e-darwin-arm64.tar.zst",
            archive_sha256: "6af749f0d44927c7d4c93c9e407e195257ed690b8e3bd3a43d7a3f4badc52082",
            archive_parts: &[],
        }),
        _ => bail!(
            "unsupported Emscripten target {platform}/{arch} in catalog commit {commit}"
        ),
    }
}

fn toolchain_root() -> Result<PathBuf> {
    Ok(fastled_root()?.join("toolchains").join("emscripten"))
}

fn install_base(spec: ToolchainSpec) -> Result<PathBuf> {
    Ok(toolchain_root()?.join(spec.platform).join(spec.arch))
}

fn package_key(spec: ToolchainSpec) -> String {
    format!("{}-{}", spec.package_id, &spec.archive_sha256[..12])
}

fn package_dir(base: &Path, spec: ToolchainSpec) -> PathBuf {
    base.join(package_key(spec))
}

fn state_path(base: &Path) -> PathBuf {
    base.join(TOOLCHAIN_STATE_FILE)
}

fn receipt_path(install: &Path) -> PathBuf {
    install.join(TOOLCHAIN_RECEIPT_FILE)
}

fn read_state(base: &Path) -> Result<ActiveToolchainState> {
    let path = state_path(base);
    if !path.is_file() {
        return Ok(ActiveToolchainState {
            schema_version: TOOLCHAIN_STATE_SCHEMA,
            ..ActiveToolchainState::default()
        });
    }
    let state = ActiveToolchainState::parse(
        &fs::read(&path).with_context(|| format!("read {}", path.display()))?,
    )
    .with_context(|| format!("parse {}", path.display()))?;
    if state.schema_version != TOOLCHAIN_STATE_SCHEMA {
        bail!(
            "unsupported toolchain state schema {}",
            state.schema_version
        );
    }
    Ok(state)
}

fn write_state(base: &Path, state: &ActiveToolchainState) -> Result<()> {
    let path = state_path(base);
    let temp = base.join(format!(".toolchain-state-{}.tmp", std::process::id()));
    fs::write(&temp, state.encode()?)?;
    if path.exists() {
        let backup = base.join(format!(".toolchain-state-{}.bak", std::process::id()));
        fs::rename(&path, &backup).with_context(|| format!("backup {}", path.display()))?;
        if let Err(error) = fs::rename(&temp, &path) {
            let _ = fs::rename(&backup, &path);
            return Err(error).with_context(|| format!("publish {}", path.display()));
        }
        let _ = fs::remove_file(backup);
    } else {
        fs::rename(&temp, &path).with_context(|| format!("publish {}", path.display()))?;
    }
    Ok(())
}

fn write_receipt(install: &Path, spec: ToolchainSpec, health_checked: bool) -> Result<()> {
    let receipt = ToolchainReceipt {
        schema_version: TOOLCHAIN_RECEIPT_SCHEMA,
        catalog_commit: TOOLCHAIN_CATALOG_COMMIT.to_string(),
        platform: spec.platform.to_string(),
        arch: spec.arch.to_string(),
        package_id: spec.package_id.to_string(),
        archive_url: spec.archive_url.to_string(),
        archive_sha256: spec.archive_sha256.to_string(),
        health_checked,
    };
    fs::write(receipt_path(install), receipt.encode()?)?;
    Ok(())
}

fn read_receipt(install: &Path) -> Result<ToolchainReceipt> {
    let path = receipt_path(install);
    ToolchainReceipt::parse(&fs::read(&path).with_context(|| format!("read {}", path.display()))?)
        .with_context(|| format!("parse {}", path.display()))
}

fn validate_managed_install(install: &Path, spec: ToolchainSpec) -> Result<()> {
    validate_complete_emscripten_install(install)?;
    let receipt = read_receipt(install)?;
    if receipt.platform != spec.platform
        || receipt.arch != spec.arch
        || receipt.package_id != spec.package_id
        || receipt.archive_sha256 != spec.archive_sha256
    {
        bail!(
            "toolchain receipt does not match catalog package {}",
            spec.package_id
        );
    }
    Ok(())
}

fn has_install_history(base: &Path) -> bool {
    fs::read_dir(base)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            entry.path().is_dir() || name == EMSCRIPTEN_VERSION_MARKER
        })
}

fn migrate_legacy_install(base: &Path, spec: ToolchainSpec) -> Result<Option<PathBuf>> {
    let mut candidates = Vec::new();
    let marker = base.join(EMSCRIPTEN_VERSION_MARKER);
    if let Ok(version) = fs::read_to_string(&marker) {
        candidates.push(base.join(version.trim()));
    }
    candidates.push(base.join(spec.package_id));
    candidates.push(base.join(spec.package_id.split('-').next().unwrap_or(spec.package_id)));

    for candidate in candidates {
        if candidate.is_dir()
            && validate_complete_emscripten_install(&candidate).is_ok()
            && candidate.file_name().is_some_and(|name| {
                name == spec.package_id || name == spec.package_id.split('-').next().unwrap_or("")
            })
        {
            write_receipt(&candidate, spec, false)?;
            let state = ActiveToolchainState {
                schema_version: TOOLCHAIN_STATE_SCHEMA,
                active: Some(
                    candidate
                        .strip_prefix(base)
                        .unwrap_or(&candidate)
                        .to_string_lossy()
                        .into_owned(),
                ),
                previous_known_good: None,
            };
            write_state(base, &state)?;
            fs::remove_file(marker).ok();
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

fn resolve_active_install(base: &Path, spec: ToolchainSpec) -> Result<Option<PathBuf>> {
    fs::create_dir_all(base)?;
    let mut state = read_state(base)?;
    if state.active.is_none() {
        if let Some(legacy) = migrate_legacy_install(base, spec)? {
            return Ok(Some(legacy));
        }
        return Ok(None);
    }

    let active = base.join(state.active.as_deref().unwrap_or_default());
    if validate_managed_install(&active, spec).is_ok() {
        return Ok(Some(active));
    }
    if let Some(previous_key) = state.previous_known_good.take() {
        let previous = base.join(&previous_key);
        if validate_managed_install(&previous, spec).is_ok() {
            return Ok(Some(previous));
        }
    }
    bail!("active Emscripten toolchain is missing or invalid; run `fastled toolchain repair`")
}

fn atomic_download(url: &str, destination: &Path) -> Result<()> {
    let partial = destination.with_extension("partial");
    if partial.exists() {
        fs::remove_file(&partial).ok();
    }
    archive::download(url, &partial).with_context(|| format!("download archive {url}"))?;
    fs::rename(&partial, destination)
        .with_context(|| format!("publish downloaded archive {}", destination.display()))?;
    Ok(())
}

fn download_multipart(
    parts: &[ToolchainPart],
    cache_dir: &Path,
    archive_path: &Path,
) -> Result<()> {
    let merged_partial = archive_path.with_extension("partial");
    let merged_file = fs::File::create(&merged_partial)
        .with_context(|| format!("create partial archive {}", merged_partial.display()))?;
    let mut merged_writer = BufWriter::new(merged_file);
    for (index, part) in parts.iter().enumerate() {
        let part_path = cache_dir.join(format!(
            "{}.part-{index:02}",
            package_key_for_archive(archive_path)
        ));
        if !part_path.is_file() || !archive::verify_sha256(&part_path, part.sha256).unwrap_or(false)
        {
            if part_path.exists() {
                fs::remove_file(&part_path).ok();
            }
            atomic_download(part.url, &part_path)?;
        }
        if !archive::verify_sha256(&part_path, part.sha256)? {
            fs::remove_file(&part_path).ok();
            bail!("checksum mismatch for multipart package part {}", index);
        }
        let mut reader = BufReader::new(fs::File::open(&part_path)?);
        std::io::copy(&mut reader, &mut merged_writer)?;
    }
    merged_writer.flush()?;
    fs::rename(&merged_partial, archive_path)
        .with_context(|| format!("publish multipart archive {}", archive_path.display()))?;
    Ok(())
}

fn package_key_for_archive(path: &Path) -> String {
    path.file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("emscripten")
        .to_string()
}

/// Return an absolute executable path when `candidate` names a file.
fn absolute_executable(candidate: &Path) -> Option<PathBuf> {
    fs::canonicalize(candidate)
        .ok()
        .filter(|path| path.is_file())
}

/// Return an existing absolute executable without resolving symlinks.
///
/// Virtual-environment Python launchers are commonly symlinks to a base
/// interpreter. Callers that need a sibling tool must retain the launcher
/// path so they remain in the selected virtual environment.
fn preserved_absolute_executable(candidate: &Path) -> Option<PathBuf> {
    (candidate.is_absolute() && candidate.is_file()).then(|| candidate.to_path_buf())
}

/// Build the path to the `uv` launcher next to a selected Python launcher.
///
/// This is deliberately a path-only helper: canonicalizing `python` first
/// would turn a virtual-environment symlink into its base interpreter and
/// cause us to look for `uv` outside the selected environment.
fn sibling_uv_executable(python: &Path) -> Option<PathBuf> {
    let uv = if cfg!(windows) { "uv.exe" } else { "uv" };
    python.parent().map(|parent| parent.join(uv))
}

/// Resolve `program` from the process PATH without ever returning a bare
/// program name.  Child tools must receive concrete paths so their behaviour
/// does not change when they spawn another process with a narrower PATH.
fn executable_from_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let suffixes: &[&str] = if cfg!(windows) {
        &[".exe", ".cmd", ".bat", ""]
    } else {
        &[""]
    };
    std::env::split_paths(&path).find_map(|directory| {
        suffixes
            .iter()
            .find_map(|suffix| absolute_executable(&directory.join(format!("{program}{suffix}"))))
    })
}

fn resolve_managed_uv() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("FASTLED_UV_EXECUTABLE").map(PathBuf::from) {
        if let Some(path) = absolute_executable(&path) {
            return Ok(path);
        }
    }
    if let Ok(python) = resolve_managed_python() {
        if let Some(candidate) = sibling_uv_executable(&python) {
            if let Some(path) = absolute_executable(&candidate) {
                return Ok(path);
            }
        }
    }
    executable_from_path("uv")
        .context("uv is required to provision FastLED's managed Python and Node environment")
}

/// Resolve the exact Python interpreter that Emscripten child processes use.
/// The wheel launcher supplies `FASTLED_PYTHON_EXECUTABLE`; a virtual
/// environment is the next best source.  Falling back to PATH still returns
/// an absolute path, not a shell-dependent `python` token.
pub(crate) fn resolve_managed_python() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("FASTLED_PYTHON_EXECUTABLE").map(PathBuf::from) {
        if let Some(path) = preserved_absolute_executable(&path) {
            return Ok(path);
        }
    }
    if let Some(venv) = std::env::var_os("VIRTUAL_ENV").map(PathBuf::from) {
        let candidate = if cfg!(windows) {
            venv.join("Scripts").join("python.exe")
        } else {
            venv.join("bin").join("python")
        };
        if let Some(path) = preserved_absolute_executable(&candidate) {
            return Ok(path);
        }
    }
    let names: &[&str] = if cfg!(windows) {
        &["python"]
    } else {
        &["python3", "python"]
    };
    names
        .iter()
        .find_map(|name| executable_from_path(name))
        .context(
            "Python is required; set FASTLED_PYTHON_EXECUTABLE or activate a virtual environment",
        )
}

/// Resolve Python from the selected FastLED uv environment while preserving
/// the virtual-environment path. Canonicalizing this symlink would produce the
/// base interpreter path and silently discard the environment's dependencies.
fn fastled_venv_executable(fastled_dir: &Path, program: &str) -> Option<PathBuf> {
    let candidate = if cfg!(windows) {
        fastled_dir
            .join(".venv")
            .join("Scripts")
            .join(format!("{program}.exe"))
    } else {
        fastled_dir.join(".venv").join("bin").join(program)
    };
    (candidate.is_file() && candidate.is_absolute()).then_some(candidate)
}

pub(crate) fn resolve_fastled_python(fastled_dir: &Path) -> Result<PathBuf> {
    let mut roots = vec![fastled_dir.to_path_buf()];
    if let Ok(root) = fastled_root() {
        roots.push(root.join("cache").join("fl").join("repo"));
        roots.push(root.join("cache").join("fastled-master"));
    }
    for root in roots {
        if let Some(python) = fastled_venv_executable(&root, "python") {
            return Ok(python);
        }
    }
    resolve_managed_python()
}

/// Resolve Node from the FastLED uv environment before considering the
/// caller's PATH. `fastled_dir` is normally the short cached checkout at
/// `~/.fastled/cache/fl/repo`; the cache fallback keeps toolchain health
/// checks aligned with normal builds.
pub(crate) fn resolve_managed_node(fastled_dir: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("FASTLED_NODE_EXECUTABLE").map(PathBuf::from) {
        if let Some(path) = absolute_executable(&path) {
            return Ok(path);
        }
    }

    let mut roots = Vec::new();
    if let Some(fastled_dir) = fastled_dir {
        roots.push(fastled_dir.to_path_buf());
    }
    if let Ok(root) = fastled_root() {
        roots.push(root.join("cache").join("fl").join("repo"));
        roots.push(root.join("cache").join("fastled-master"));
    }
    for root in roots {
        let candidate = if cfg!(windows) {
            root.join(".venv").join("Scripts").join("node.exe")
        } else {
            root.join(".venv").join("bin").join("node")
        };
        if let Some(path) = absolute_executable(&candidate) {
            return Ok(path);
        }
    }

    executable_from_path("node")
        .context("Node.js is required; FastLED's uv environment did not provide .venv/bin/node and node was not found on PATH")
}

/// Ensure the selected FastLED checkout has the uv-managed runtime required by
/// its Meson helpers and Emscripten. This is intentionally project-local and
/// never modifies the user's global Python, Node, or shell PATH.
pub(crate) fn ensure_fastled_uv_environment(fastled_dir: &Path) -> Result<()> {
    if fastled_venv_executable(fastled_dir, "python").is_some()
        && fastled_venv_executable(fastled_dir, "node").is_some()
    {
        return Ok(());
    }
    if !fastled_dir.join("pyproject.toml").is_file() {
        // Older pinned FastLED releases predate the uv project. They can still
        // use an explicitly supplied or ambient Node runtime.
        resolve_managed_node(Some(fastled_dir))?;
        return Ok(());
    }
    let uv = resolve_managed_uv()?;
    let python = resolve_managed_python()?;
    let mut command = Command::new(&uv);
    command
        .args(["sync", "--no-dev", "--project"])
        .arg(fastled_dir)
        .arg("--python")
        .arg(&python)
        .env("FASTLED_PYTHON_EXECUTABLE", &python);
    let status = command
        .status()
        .with_context(|| format!("provision FastLED runtime with {}", uv.display()))?;
    if !status.success() {
        bail!("uv failed to provision FastLED runtime with {status}");
    }
    for program in ["python", "node"] {
        fastled_venv_executable(fastled_dir, program).with_context(|| {
            format!(
                "uv completed but did not provide {program} in {}",
                fastled_dir.join(".venv").display()
            )
        })?;
    }
    Ok(())
}

fn prepend_runtime_path(command: &mut Command, python: &Path, node: &Path) {
    let mut entries = Vec::new();
    if let Some(parent) = python.parent() {
        entries.push(parent.to_path_buf());
    }
    if let Some(parent) = node.parent() {
        entries.push(parent.to_path_buf());
    }
    if let Some(path) = std::env::var_os("PATH") {
        entries.extend(std::env::split_paths(&path));
    }
    if let Ok(path) = std::env::join_paths(entries) {
        command.env("PATH", path);
    }
}

fn run_health_checks(install: &Path) -> Result<()> {
    let temp =
        std::env::temp_dir().join(format!("fastled-toolchain-health-{}", std::process::id()));
    if temp.exists() {
        fs::remove_dir_all(&temp).ok();
    }
    fs::create_dir_all(&temp)?;
    let result = (|| -> Result<()> {
        let empp = install.join("emscripten").join("em++.py");
        let python = resolve_managed_python()?;
        let node = resolve_managed_node(None)?;
        // Older managed installs may retain their pre-publication staging path.
        // Restore our generated configuration before probing the actual install.
        archive::write_emscripten_config(install, &node)?;
        let run = |args: &[&str]| -> Result<()> {
            let mut command = Command::new(&python);
            command
                .arg(&empp)
                .args(args)
                .env("EM_CONFIG", install.join(".emscripten"))
                .env("EMSCRIPTEN", install.join("emscripten"))
                .env("EMSDK_PYTHON", &python)
                .env("FASTLED_PYTHON_EXECUTABLE", &python)
                .current_dir(&temp);
            prepend_runtime_path(&mut command, &python, &node);
            let status = command.status().context("run Emscripten health check")?;
            if !status.success() {
                bail!(
                    "Emscripten health-check command failed: em++ {}",
                    args.join(" ")
                );
            }
            Ok(())
        };
        fs::write(temp.join("static.cpp"), "int main() { return 0; }\n")?;
        run(&[
            "static.cpp",
            "-O0",
            "-sWASM_BIGINT=1",
            "-sEXIT_RUNTIME=1",
            "-o",
            "static.js",
        ])?;
        let static_status = Command::new(&node)
            .arg(temp.join("static.js"))
            .current_dir(&temp)
            .status()
            .context("run static Emscripten health check")?;
        if !static_status.success() {
            bail!("static Emscripten health check failed");
        }

        fs::write(temp.join("side.cpp"), "int side_value() { return 42; }\n")?;
        fs::write(
            temp.join("main.cpp"),
            "extern int side_value(); int main() { return side_value() == 42 ? 0 : 1; }\n",
        )?;
        run(&[
            "side.cpp",
            "-O0",
            "-sSIDE_MODULE=1",
            "-sWASM_BIGINT=1",
            "-o",
            "side.wasm",
        ])?;
        run(&[
            "main.cpp",
            "side.wasm",
            "-O0",
            "-sMAIN_MODULE=2",
            "-sWASM_BIGINT=1",
            "-sEXIT_RUNTIME=1",
            "-o",
            "dynamic.js",
        ])?;
        let dynamic_status = Command::new(&node)
            .arg(temp.join("dynamic.js"))
            .current_dir(&temp)
            .status()
            .context("run dynamic Emscripten health check")?;
        if !dynamic_status.success() {
            bail!("dynamic Emscripten health check failed");
        }
        Ok(())
    })();
    fs::remove_dir_all(&temp).ok();
    result
}

fn install_spec(
    spec: ToolchainSpec,
    health_check: bool,
    replace_invalid: bool,
    force_redownload: bool,
) -> Result<PathBuf> {
    let base = install_base(spec)?;
    let root = fastled_root()?;
    let cache_dir = root.join("toolchains").join("archives");
    fs::create_dir_all(&base)?;
    fs::create_dir_all(&cache_dir)?;
    let destination = package_dir(&base, spec);
    if !force_redownload && validate_managed_install(&destination, spec).is_ok() {
        if health_check {
            run_health_checks(&destination)?;
        }
        return Ok(destination);
    }
    let archive_path = cache_dir.join(format!("emscripten-{}.tar.zst", package_key(spec)));
    if force_redownload && archive_path.exists() {
        fs::remove_file(&archive_path).ok();
    }
    if !archive_path.is_file()
        || !archive::verify_sha256(&archive_path, spec.archive_sha256).unwrap_or(false)
    {
        if archive_path.exists() {
            fs::remove_file(&archive_path).ok();
        }
        if spec.archive_parts.is_empty() {
            atomic_download(spec.archive_url, &archive_path)?;
        } else {
            download_multipart(spec.archive_parts, &cache_dir, &archive_path)?;
        }
    }
    if !archive::verify_sha256(&archive_path, spec.archive_sha256)? {
        fs::remove_file(&archive_path).ok();
        bail!("checksum mismatch for catalog package {}", spec.package_id);
    }

    let node = resolve_managed_node(None)?;
    let staging = base.join(format!(
        ".{}.staging-{}",
        package_key(spec),
        std::process::id()
    ));
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;
    let result = (|| -> Result<()> {
        archive::extract_tar_zst(&archive_path, &staging)?;
        ensure_toolchain_executables(&staging)?;
        validate_emscripten_payload(&staging)?;
        archive::write_emscripten_config(&staging, &node)?;
        fs::write(staging.join("done.txt"), "ok\n")?;
        write_receipt(&staging, spec, false)?;
        validate_complete_emscripten_install(&staging)?;
        if health_check {
            run_health_checks(&staging)?;
            write_receipt(&staging, spec, true)?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        fs::remove_dir_all(&staging).ok();
        return Err(error);
    }
    if destination.exists() {
        if !replace_invalid {
            fs::remove_dir_all(&staging).ok();
            bail!(
                "catalog package {} is present but invalid; run repair",
                spec.package_id
            );
        }
        let quarantine = base.join(format!(
            ".{}.invalid-{}",
            package_key(spec),
            std::process::id()
        ));
        fs::rename(&destination, &quarantine)
            .with_context(|| format!("quarantine invalid toolchain {}", destination.display()))?;
    }
    publish_toolchain(&staging, &destination, &node)?;
    Ok(destination)
}

fn publish_toolchain(staging: &Path, destination: &Path, node: &Path) -> Result<()> {
    // Health checks ran against staging. Rewrite only our generated config for
    // the destination before the directory rename makes the package visible.
    archive::write_emscripten_config_at(staging, destination, node)?;
    fs::rename(staging, destination)
        .with_context(|| format!("publish toolchain {}", destination.display()))?;
    Ok(())
}

fn activate_install(base: &Path, install: &Path, spec: ToolchainSpec) -> Result<()> {
    validate_managed_install(install, spec)?;
    let mut state = read_state(base)?;
    let key = install
        .strip_prefix(base)
        .unwrap_or(install)
        .to_string_lossy()
        .into_owned();
    if state.active.as_deref() != Some(key.as_str()) {
        state.previous_known_good = state.active.take();
        state.active = Some(key);
    }
    state.schema_version = TOOLCHAIN_STATE_SCHEMA;
    write_state(base, &state)
}

fn find_installed_package(base: &Path, package_id: &str, spec: ToolchainSpec) -> Result<PathBuf> {
    for entry in fs::read_dir(base)? {
        let path = entry?.path();
        if path.is_dir()
            && read_receipt(&path).is_ok_and(|receipt| receipt.package_id == package_id)
            && validate_managed_install(&path, spec).is_ok()
        {
            return Ok(path);
        }
    }
    bail!("supported package {package_id} is not installed; run `fastled toolchain install`")
}

fn with_toolchain_lock<T>(base: &Path, action: impl FnOnce() -> Result<T>) -> Result<T> {
    let lock_path = base.join(".fastled-toolchain.lock");
    let lock = fs::File::create(&lock_path)
        .with_context(|| format!("create toolchain lock {}", lock_path.display()))?;
    let guard = kernal_api::platform::fs::lock_exclusive(&lock)
        .with_context(|| format!("lock toolchain state {}", lock_path.display()))?;
    let result = action();
    drop(guard);
    drop(lock);
    result
}

fn run_toolchain_action_locked(
    action: crate::cli::ToolchainAction,
    spec: ToolchainSpec,
    base: &Path,
) -> Result<()> {
    match action {
        crate::cli::ToolchainAction::Status => {
            let active_result = resolve_active_install(base, spec);
            let state = read_state(base)?;
            println!("target: {}/{}", spec.platform, spec.arch);
            println!("release default: {}", spec.package_id);
            println!("active: {}", state.active.as_deref().unwrap_or("none"));
            println!(
                "previous known-good: {}",
                state.previous_known_good.as_deref().unwrap_or("none")
            );
            match active_result {
                Ok(Some(path)) => println!("active state: healthy ({})", path.display()),
                Ok(None) => println!("active state: not installed"),
                Err(error) => println!("active state: invalid ({error:#})"),
            }
        }
        crate::cli::ToolchainAction::Install { package_id } => {
            if package_id
                .as_deref()
                .is_some_and(|id| id != spec.package_id)
            {
                bail!(
                    "package is not in this CLI release catalog: {}",
                    package_id.unwrap()
                );
            }
            let path = install_spec(spec, false, false, false)?;
            println!("installed {} at {}", spec.package_id, path.display());
        }
        crate::cli::ToolchainAction::Activate { package_id } => {
            if package_id != spec.package_id {
                bail!("package is not in this CLI release catalog: {package_id}");
            }
            let path = find_installed_package(base, &package_id, spec)?;
            run_health_checks(&path)?;
            activate_install(base, &path, spec)?;
            println!("activated {}", package_id);
        }
        crate::cli::ToolchainAction::Update => {
            let path = install_spec(spec, true, true, false)?;
            activate_install(base, &path, spec)?;
            println!("updated and activated {}", spec.package_id);
        }
        crate::cli::ToolchainAction::Repair { package_id } => {
            if package_id
                .as_deref()
                .is_some_and(|id| id != spec.package_id)
            {
                bail!(
                    "package is not in this CLI release catalog: {}",
                    package_id.unwrap()
                );
            }
            let path = install_spec(spec, true, true, true)?;
            activate_install(base, &path, spec)?;
            println!("repaired and activated {}", spec.package_id);
        }
        crate::cli::ToolchainAction::Rollback => {
            let mut state = read_state(base)?;
            let previous = state
                .previous_known_good
                .clone()
                .context("no previous known-good toolchain is recorded")?;
            let path = base.join(&previous);
            validate_managed_install(&path, spec)?;
            let old_active = state.active.take();
            state.active = Some(previous);
            state.previous_known_good = old_active;
            write_state(base, &state)?;
            println!("rolled back toolchain");
        }
        crate::cli::ToolchainAction::Prune => {
            let state = read_state(base)?;
            let keep = [
                state.active.as_deref(),
                state.previous_known_good.as_deref(),
            ];
            let mut removed = 0;
            for entry in fs::read_dir(base)? {
                let path = entry?.path();
                let key = path.file_name().and_then(|name| name.to_str());
                if path.is_dir() && key.is_some() && !keep.contains(&key) {
                    fs::remove_dir_all(path)?;
                    removed += 1;
                }
            }
            println!("pruned {removed} inactive toolchain installation(s)");
        }
    }
    Ok(())
}

pub(crate) fn run_toolchain_action(action: crate::cli::ToolchainAction) -> Result<()> {
    if matches!(
        &action,
        crate::cli::ToolchainAction::Install { .. }
            | crate::cli::ToolchainAction::Activate { .. }
            | crate::cli::ToolchainAction::Update
            | crate::cli::ToolchainAction::Repair { .. }
    ) {
        let fastled_dir = ensure_fastled_repo(Some("master"))?;
        ensure_fastled_uv_environment(&fastled_dir)?;
    }
    let spec = release_default_toolchain()?;
    let base = install_base(spec)?;
    fs::create_dir_all(&base)?;
    with_toolchain_lock(&base, || run_toolchain_action_locked(action, spec, &base))
}

/// Ensure the catalog-selected Emscripten toolchain is active.
///
/// Normal compilation never consults a remote manifest. The only implicit
/// download is a first-run bootstrap when no installation history exists.
pub fn ensure_emscripten_installed() -> Result<PathBuf> {
    let cache = EMSCRIPTEN_INSTALL_CACHE.get_or_init(|| Mutex::new(None));
    let mut cached = cache
        .lock()
        .map_err(|_| anyhow::anyhow!("emscripten install cache lock poisoned"))?;
    if let Some(path) = cached.clone() {
        if validate_complete_emscripten_install(&path).is_ok() {
            return Ok(path);
        }
        *cached = None;
    }
    drop(cached);

    let spec = release_default_toolchain()?;
    let base = install_base(spec)?;
    fs::create_dir_all(&base)?;
    let installed = with_toolchain_lock(&base, || match resolve_active_install(&base, spec)? {
        Some(path) => Ok(path),
        None if !has_install_history(&base) => {
            let path = install_spec(spec, true, false, false)?;
            activate_install(&base, &path, spec)?;
            Ok(path)
        }
        None => {
            bail!("no supported Emscripten toolchain is active; run `fastled toolchain update`")
        }
    })?;
    let mut cached = cache
        .lock()
        .map_err(|_| anyhow::anyhow!("emscripten install cache lock poisoned"))?;
    *cached = Some(installed.clone());
    Ok(installed)
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
    let runtime = kernal_api::async_engine::RuntimeBuilder::current_thread()
        .enable_all()
        .build()
        .ok()?;
    let client = kernal_api::http::BlockingClient::new(
        &runtime,
        kernal_api::http::Limits {
            max_redirects: 10,
            total_timeout: std::time::Duration::from_secs(10),
            ..kernal_api::http::Limits::default()
        },
    )
    .ok()?;
    let resp = client
        .execute(kernal_api::http::Request {
            headers: &[
                ("Accept", "application/vnd.github.v3+json"),
                ("User-Agent", "fastled-cli"),
            ],
            ..kernal_api::http::Request::get(FASTLED_LATEST_RELEASE_API)
        })
        .ok()?;
    if !(200..300).contains(&resp.status()) {
        return None;
    }
    let bytes = resp.into_bytes().ok()?;
    let JsonValue::Object(mut value) = json::parse(&bytes).ok()? else {
        return None;
    };
    match value.remove("tag_name") {
        Some(JsonValue::String(value)) => Some(value),
        _ => None,
    }
}

fn head_check(url: &str) -> bool {
    let Ok(runtime) = kernal_api::async_engine::RuntimeBuilder::current_thread()
        .enable_all()
        .build()
    else {
        return false;
    };
    let client = kernal_api::http::BlockingClient::new(
        &runtime,
        kernal_api::http::Limits {
            max_redirects: 10,
            total_timeout: std::time::Duration::from_secs(10),
            ..kernal_api::http::Limits::default()
        },
    );
    match client {
        Ok(c) => c
            .execute(kernal_api::http::Request {
                method: kernal_api::http::Method::Head,
                headers: &[("User-Agent", "fastled-cli")],
                ..kernal_api::http::Request::get(url)
            })
            .map(|r| (200..300).contains(&r.status()))
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

/// Remove the derived short-path checkout when it represents `reference`.
/// The authoritative source checkout remains untouched.
pub(crate) fn invalidate_short_fastled_copy(reference: &str) -> Result<()> {
    let cache_base = fastled_root()?.join("cache");
    let authoritative = crate::source::repo_dir(&cache_base, reference)?;
    let short_root = cache_base.join("fl");
    let marker = short_root.join("repo").join(".fastled-source");
    let matches = fs::read_to_string(&marker)
        .map(|value| value.lines().next() == Some(authoritative.to_string_lossy().as_ref()))
        .unwrap_or(false);
    if matches && short_root.exists() {
        fs::remove_dir_all(&short_root).with_context(|| {
            format!(
                "invalidate derived FastLED checkout {}",
                short_root.display()
            )
        })?;
    }
    Ok(())
}

/// Force-refresh one explicitly named FastLED ref using staged publication.
pub(crate) fn refresh_fastled_repo(reference: &str) -> Result<PathBuf> {
    let (ref_name, url) = resolve_fastled_ref(Some(reference));
    if ref_name != reference {
        bail!("requested FastLED ref {reference:?} resolved unexpectedly as {ref_name:?}");
    }
    let cache_base = fastled_root()?.join("cache");
    let archive_cache = cache_base.join("archives");
    fs::create_dir_all(&archive_cache)?;
    let download_dir = kernal_api::platform::fs::TemporaryDirectory::in_directory(
        &archive_cache,
        "fastled-source-download-",
    )
    .context("create temporary FastLED download directory")?;
    let archive_path = download_dir.path().join("FastLED.zip");
    archive::download(&url, &archive_path)
        .with_context(|| format!("download FastLED archive from {url}"))?;
    let receipt =
        crate::source::SourceReceipt::new(&ref_name, &url, None, std::time::SystemTime::now())?;
    let repo = crate::source::update_checkout(&cache_base, &receipt, |staging| {
        let unpacked = staging.join(".unpacked");
        archive::extract_zip(&archive_path, &unpacked)?;
        let extracted_root = find_fastled_extract_root(&unpacked)?;
        for entry in fs::read_dir(&extracted_root)? {
            let entry = entry?;
            fs::rename(entry.path(), staging.join(entry.file_name())).with_context(|| {
                format!("promote FastLED archive entry {}", entry.path().display())
            })?;
        }
        fs::remove_dir_all(&unpacked)?;
        Ok(())
    })?;
    // A prior short checkout has the same authoritative path in its marker;
    // remove it so the next build cannot silently reuse old source bytes.
    invalidate_short_fastled_copy(&ref_name)?;
    Ok(repo.into_path_buf())
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

    if ref_name == "master" {
        return refresh_fastled_repo("master");
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

    let receipt =
        crate::source::SourceReceipt::new(&ref_name, &url, None, std::time::SystemTime::now())?;
    crate::source::write_receipt(&repo_dir, &receipt)?;

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
    let Ok(JsonValue::Object(value)) = json::parse(text.as_bytes()) else {
        return false;
    };
    matches!(value.get("name"), Some(JsonValue::String(name)) if name == "FastLED")
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
    let Ok(JsonValue::Object(value)) = json::parse(text.as_bytes()) else {
        return false;
    };
    if !matches!(value.get("name"), Some(JsonValue::String(name)) if name == "FastLED") {
        return false;
    }
    let Some(JsonValue::Object(repository)) = value.get("repository") else {
        return false;
    };
    if !matches!(repository.get("url"), Some(JsonValue::String(url)) if url.contains("FastLED/FastLED"))
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

fn read_editor_object(
    path: &Path,
    default: BTreeMap<String, JsonValue>,
) -> Result<BTreeMap<String, JsonValue>> {
    let Ok(bytes) = fs::read(path) else {
        return Ok(default);
    };
    match json::parse(&bytes) {
        Ok(JsonValue::Object(value)) => Ok(value),
        Ok(_) | Err(json::Error::InvalidSyntax) => Ok(default),
        Err(error) => Err(error).with_context(|| format!("parse {}", path.display())),
    }
}

fn write_json_file(path: &Path, value: &JsonValue) -> Result<()> {
    let mut bytes = json::encode(value, Layout::Pretty).context("serialize JSON")?;
    bytes.push(b'\n');
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(path, bytes).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn editor_templates() -> Result<BTreeMap<String, JsonValue>> {
    match json::parse(include_bytes!("install_editor_templates.json"))? {
        JsonValue::Object(value) => Ok(value),
        _ => bail!("invalid bundled editor templates"),
    }
}

fn update_launch_json_for_arduino() -> Result<()> {
    let cwd = std::env::current_dir().context("current dir")?;
    update_launch_json_at(&cwd.join(".vscode").join("launch.json"))
}

fn update_launch_json_at(path: &Path) -> Result<()> {
    let mut data = read_editor_object(
        path,
        BTreeMap::from([
            ("version".into(), JsonValue::String("0.2.0".into())),
            ("configurations".into(), JsonValue::Array(Vec::new())),
        ]),
    )?;
    let arduino_config = editor_templates()?
        .remove("launch")
        .context("missing bundled launch configuration")?;
    let mut configurations = match data.remove("configurations") {
        Some(JsonValue::Array(value)) => value,
        _ => Vec::new(),
    };
    let exists = configurations.iter().any(|value| match value {
        JsonValue::Object(value) => matches!(value.get("name"),
            Some(JsonValue::String(name)) if name == "Auto Debug (Smart File Detection)"),
        _ => false,
    });
    if !exists {
        configurations.insert(0, arduino_config);
    }
    data.insert("configurations".into(), JsonValue::Array(configurations));
    write_json_file(path, &JsonValue::Object(data))?;
    println!("Updated {}", path.display());
    Ok(())
}

fn generate_fastled_tasks() -> Result<()> {
    let cwd = std::env::current_dir().context("current dir")?;
    generate_fastled_tasks_at(&cwd.join(".vscode").join("tasks.json"))
}

fn generate_fastled_tasks_at(path: &Path) -> Result<()> {
    let mut data = read_editor_object(
        path,
        BTreeMap::from([
            ("version".into(), JsonValue::String("2.0.0".into())),
            ("tasks".into(), JsonValue::Array(Vec::new())),
        ]),
    )?;
    let Some(JsonValue::Array(fastled_tasks)) = editor_templates()?.remove("tasks") else {
        bail!("missing bundled FastLED tasks");
    };
    let mut tasks = match data.remove("tasks") {
        Some(JsonValue::Array(value)) => value,
        _ => Vec::new(),
    };
    let labels: Vec<_> = tasks
        .iter()
        .filter_map(|value| match value {
            JsonValue::Object(value) => match value.get("label") {
                Some(JsonValue::String(value)) => Some(value.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    for task in fastled_tasks {
        let JsonValue::Object(ref fields) = task else {
            bail!("invalid bundled task");
        };
        let Some(JsonValue::String(label)) = fields.get("label") else {
            bail!("missing bundled task label");
        };
        if !labels.contains(label) {
            tasks.push(task);
        }
    }
    data.insert("tasks".into(), JsonValue::Array(tasks));
    write_json_file(path, &JsonValue::Object(data))?;
    println!("Updated {}", path.display());
    Ok(())
}

fn fastled_repository_settings() -> Result<JsonValue> {
    json::parse(include_bytes!("install_repository_settings.json"))
        .context("parse bundled FastLED repository settings")
}

fn update_vscode_settings_for_fastled() -> Result<()> {
    if !is_fastled_repository() {
        return Ok(());
    }

    let cwd = std::env::current_dir().context("current dir")?;
    let settings_json_path = cwd.join(".vscode").join("settings.json");
    update_repository_settings_at(&settings_json_path)?;
    println!(
        "Updated {} with comprehensive FastLED development settings",
        settings_json_path.display()
    );
    Ok(())
}

fn update_repository_settings_at(path: &Path) -> Result<()> {
    let mut data = read_editor_object(path, BTreeMap::new())?;
    let JsonValue::Object(settings) = fastled_repository_settings()? else {
        bail!("invalid bundled repository settings");
    };
    data.extend(settings);
    write_json_file(path, &JsonValue::Object(data))
}

fn download_to_path(url: &str, dest: &Path) -> Result<()> {
    archive::download(url, dest)
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

    let temp_dir = kernal_api::platform::fs::TemporaryDirectory::new()
        .context("create temp dir for extension")?;
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
    #[test]
    fn published_toolchain_config_does_not_retain_staging_paths() {
        let temp = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        let staging = temp.path().join(".toolchain.staging");
        let destination = temp.path().join("toolchain");
        let node = temp.path().join("node");
        std::fs::create_dir_all(&staging).unwrap();
        crate::archive::write_emscripten_config(&staging, &node).unwrap();
        super::publish_toolchain(&staging, &destination, &node).unwrap();
        let config = std::fs::read_to_string(destination.join(".emscripten")).unwrap();
        assert!(!config.contains(".toolchain.staging"), "{config}");
        assert!(config.contains(&destination.to_string_lossy().replace('\\', "/")));
        assert!(!staging.exists());
    }

    #[test]
    fn invalid_config_does_not_publish_toolchain() {
        let temp = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        let staging = temp.path().join("staging");
        let destination = temp.path().join("final");
        std::fs::create_dir_all(&staging).unwrap();
        assert!(
            super::publish_toolchain(&staging, &destination, std::path::Path::new("node")).is_err()
        );
        assert!(staging.exists());
        assert!(!destination.exists());
    }

    #[test]
    fn toolchain_target_aliases_preserve_supported_binary_targets() {
        for (os, platform) in [("windows", "win"), ("linux", "linux"), ("macos", "darwin")] {
            for (architecture, arch) in [("x86_64", "x86_64"), ("aarch64", "arm64")] {
                assert_eq!(
                    super::toolchain_platform_arch(os, architecture).unwrap(),
                    (platform, arch)
                );
            }
        }
        assert_eq!(
            super::toolchain_platform_arch("freebsd", "x86_64")
                .unwrap_err()
                .to_string(),
            "unsupported operating system"
        );
        assert_eq!(
            super::toolchain_platform_arch("linux", "x86")
                .unwrap_err()
                .to_string(),
            "unsupported architecture: x86"
        );
    }

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

    fn test_spec() -> ToolchainSpec {
        ToolchainSpec {
            platform: "test-platform",
            arch: "test-arch",
            package_id: "4.0.19",
            archive_url: "https://example.com/emscripten.tar.zst",
            archive_sha256: "b19c2e35b863eb17866034f917d7957514645e179e9d22800729b0dcbb2aa2e2",
            archive_parts: &[],
        }
    }

    fn create_valid_emscripten_install(root: &Path) -> PathBuf {
        let spec = test_spec();
        let install = package_dir(root, spec);
        for path in required_emscripten_payload_files(&install) {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(
                &path,
                if path.ends_with("emscripten-version.txt") {
                    br#"\"4.0.19\"\n"#.as_slice()
                } else {
                    b"tool\n".as_slice()
                },
            )
            .unwrap();
        }
        fs::write(install.join("done.txt"), "ok\n").unwrap();
        write_receipt(&install, spec, true).unwrap();
        install
    }

    #[cfg(unix)]
    #[test]
    fn fastled_python_keeps_virtual_environment_symlink_path() {
        use std::os::unix::fs::symlink;

        let temp = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        let base_python = temp.path().join("base-python");
        fs::write(&base_python, "python").unwrap();
        let venv_bin = temp.path().join("FastLED/.venv/bin");
        fs::create_dir_all(&venv_bin).unwrap();
        let venv_python = venv_bin.join("python");
        symlink(&base_python, &venv_python).unwrap();

        assert_eq!(
            resolve_fastled_python(&temp.path().join("FastLED")).unwrap(),
            venv_python
        );
    }

    #[cfg(unix)]
    #[test]
    fn selected_virtualenv_python_keeps_sibling_uv_path() {
        use std::os::unix::fs::symlink;

        let temp = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        let base_python = temp.path().join("base/bin/python");
        fs::create_dir_all(base_python.parent().unwrap()).unwrap();
        fs::write(&base_python, "python").unwrap();

        let venv_bin = temp.path().join("venv/bin");
        fs::create_dir_all(&venv_bin).unwrap();
        let venv_python = venv_bin.join("python");
        symlink(&base_python, &venv_python).unwrap();
        let venv_uv = venv_bin.join("uv");
        fs::write(&venv_uv, "uv").unwrap();

        let selected = preserved_absolute_executable(&venv_python).unwrap();
        assert_eq!(selected, venv_python);
        assert_eq!(sibling_uv_executable(&selected), Some(venv_uv));
    }

    #[test]
    fn parses_nested_versions_manifest_without_parts() {
        let manifest =
            parse_platform_manifest(DARWIN_ARM64_MANIFEST).expect("parse darwin/arm64 manifest");
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
        assert!(multipart_parts(entry).is_none());
    }

    #[test]
    // Regression coverage for issue #194: a valid active install must not
    // fall through to manifest discovery or an implicit replacement.
    fn active_install_is_selected_without_network_or_manifest_state() {
        let temp = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        let spec = test_spec();
        let install = create_valid_emscripten_install(temp.path());
        write_state(
            temp.path(),
            &ActiveToolchainState {
                schema_version: TOOLCHAIN_STATE_SCHEMA,
                active: Some(package_key(spec)),
                previous_known_good: None,
            },
        )
        .unwrap();

        assert_eq!(
            resolve_active_install(temp.path(), spec).unwrap(),
            Some(install)
        );
    }

    #[test]
    fn toolchain_json_schema_preserves_defaults_duplicates_and_sequences() {
        let temp = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        for source in [
            r#"{"schema_version":1}"#,
            r#"{"schema_version":1,"active":null,"previous_known_good":null,"unknown":false}"#,
            r#"[1,null,null]"#,
        ] {
            fs::write(state_path(temp.path()), source).unwrap();
            assert_eq!(
                read_state(temp.path()).unwrap(),
                ActiveToolchainState {
                    schema_version: 1,
                    active: None,
                    previous_known_good: None,
                }
            );
        }
        for source in [
            r#"{"schema_version":1,"active":null,"active":"a"}"#,
            r#"{"schema_version":1,"previous_known_good":null,"previous_known_good":null}"#,
            r#"{"schema_version":1,"schema_version":1}"#,
            r#"{"schema_version":1,"active":false}"#,
            r#"{"schema_version":1.0}"#,
            r#"{"schema_version":-1}"#,
            r#"{"schema_version":4294967296}"#,
            r#"{}"#,
            r#"[1]"#,
            r#"[1,null,null,null]"#,
        ] {
            fs::write(state_path(temp.path()), source).unwrap();
            assert!(read_state(temp.path()).is_err(), "{source}");
        }
        let receipt = r#"{"schema_version":4294967295,"platform":"linux","arch":"x86_64","package_id":"λ","archive_url":"u","archive_sha256":"h","health_checked":false}"#;
        fs::write(receipt_path(temp.path()), receipt).unwrap();
        let expected = read_receipt(temp.path()).unwrap();
        assert_eq!(expected.schema_version, u32::MAX);
        assert_eq!(expected.catalog_commit, "");
        fs::write(
            receipt_path(temp.path()),
            r#"[4294967295,"","linux","x86_64","λ","u","h",false]"#,
        )
        .unwrap();
        assert_eq!(read_receipt(temp.path()).unwrap(), expected);
        for source in [
            receipt.replace("\"health_checked\":false", "\"health_checked\":null"),
            receipt.replace("\"health_checked\":false", "\"health_checked\":0"),
            receipt.replace(
                "\"health_checked\":false",
                "\"health_checked\":false,\"health_checked\":true",
            ),
            receipt.replace("\"platform\":\"linux\"", "\"platform\":null"),
            receipt.replace("\"platform\":\"linux\",", ""),
            receipt.replace(
                "\"platform\":\"linux\"",
                "\"platform\":\"linux\",\"catalog_commit\":null",
            ),
            r#"[1,"linux","x86_64","p","u","h",true]"#.to_string(),
        ] {
            fs::write(receipt_path(temp.path()), &source).unwrap();
            assert!(read_receipt(temp.path()).is_err(), "{source}");
        }
    }

    #[test]
    fn toolchain_json_encoding_preserves_wire_format_and_failed_writes() {
        let temp = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        let mut state = ActiveToolchainState {
            schema_version: 1,
            active: Some("λ".into()),
            previous_known_good: None,
        };
        write_state(temp.path(), &state).unwrap();
        let before = fs::read(state_path(temp.path())).unwrap();
        assert_eq!(
            String::from_utf8(before.clone()).unwrap(),
            "{\n  \"schema_version\": 1,\n  \"active\": \"λ\",\n  \"previous_known_good\": null\n}"
        );
        state.active = Some("x".repeat(json::MAX_OUTPUT_BYTES));
        assert!(matches!(
            write_state(temp.path(), &state)
                .unwrap_err()
                .downcast_ref::<json::Error>(),
            Some(json::Error::OutputTooLarge)
        ));
        assert_eq!(fs::read(state_path(temp.path())).unwrap(), before);
        assert!(!temp
            .path()
            .join(format!(".toolchain-state-{}.tmp", std::process::id()))
            .exists());
        let receipt = ToolchainReceipt {
            schema_version: 1,
            catalog_commit: "c".into(),
            platform: "p".into(),
            arch: "a".into(),
            package_id: "i".into(),
            archive_url: "u".into(),
            archive_sha256: "h".into(),
            health_checked: true,
        };
        let bytes = receipt.encode().unwrap();
        assert_eq!(String::from_utf8(bytes.clone()).unwrap(),
            "{\n  \"schema_version\": 1,\n  \"catalog_commit\": \"c\",\n  \"platform\": \"p\",\n  \"arch\": \"a\",\n  \"package_id\": \"i\",\n  \"archive_url\": \"u\",\n  \"archive_sha256\": \"h\",\n  \"health_checked\": true\n}");
        assert_eq!(ToolchainReceipt::parse(&bytes).unwrap(), receipt);
        let oversized = vec![b' '; json::MAX_INPUT_BYTES + 1];
        assert!(matches!(
            ToolchainReceipt::parse(&oversized)
                .unwrap_err()
                .downcast_ref::<json::Error>(),
            Some(json::Error::InputTooLarge)
        ));
        assert!(matches!(
            ActiveToolchainState::parse(&oversized)
                .unwrap_err()
                .downcast_ref::<json::Error>(),
            Some(json::Error::InputTooLarge)
        ));
    }

    #[test]
    fn legacy_marker_migrates_to_receipt_and_active_state() {
        let temp = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        let spec = test_spec();
        let install = temp.path().join(spec.package_id);
        for path in required_emscripten_payload_files(&install) {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, b"tool\n").unwrap();
        }
        fs::write(install.join("done.txt"), "ok\n").unwrap();
        fs::write(temp.path().join(EMSCRIPTEN_VERSION_MARKER), "4.0.19\n").unwrap();

        assert_eq!(
            resolve_active_install(temp.path(), spec).unwrap(),
            Some(install.clone())
        );
        assert!(receipt_path(&install).is_file());
        assert_eq!(
            read_state(temp.path()).unwrap().active,
            Some("4.0.19".to_string())
        );
    }

    #[test]
    fn existing_history_never_bootstraps_implicitly() {
        let temp = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        fs::create_dir(temp.path().join("4.0.18")).unwrap();
        assert!(has_install_history(temp.path()));
    }

    #[test]
    fn invalid_active_uses_previous_known_good_without_rewriting_state() {
        let temp = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        let spec = test_spec();
        let install = create_valid_emscripten_install(temp.path());
        let broken = temp.path().join("broken");
        fs::create_dir(&broken).unwrap();
        let state = ActiveToolchainState {
            schema_version: TOOLCHAIN_STATE_SCHEMA,
            active: Some("broken".to_string()),
            previous_known_good: Some(package_key(spec)),
        };
        write_state(temp.path(), &state).unwrap();

        assert_eq!(
            resolve_active_install(temp.path(), spec).unwrap(),
            Some(install)
        );
        assert_eq!(read_state(temp.path()).unwrap(), state);
    }

    #[test]
    fn missing_required_tool_rejects_managed_install() {
        let temp = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        let spec = test_spec();
        let install = create_valid_emscripten_install(temp.path());
        fs::remove_file(install.join(if cfg!(windows) {
            "bin/wasm-ld.exe"
        } else {
            "bin/wasm-ld"
        }))
        .unwrap();

        assert!(validate_complete_emscripten_install(&install).is_err());
        assert!(validate_managed_install(&install, spec).is_err());
    }

    #[test]
    fn parses_manifest_with_multipart_archive_and_extra_size_field() {
        let manifest = parse_platform_manifest(LINUX_X86_64_MANIFEST_WITH_PARTS)
            .expect("parse linux/x86_64 manifest");
        let entry = manifest.versions.get("4.0.21").expect("entry for 4.0.21");
        let parts = multipart_parts(entry).expect("parts present");
        assert_eq!(parts.len(), 1);
        assert!(parts[0].href.ends_with(".part-aa"));
        assert!(!parts[0].sha256.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn ensure_toolchain_executables_restores_unix_execute_bits() {
        use std::os::unix::fs::PermissionsExt;

        let temp = kernal_api::platform::fs::TemporaryDirectory::new().expect("tempdir");
        let bin_dir = temp.path().join("bin");
        fs::create_dir_all(&bin_dir).expect("create bin");
        let tool = bin_dir.join("llvm-ar");
        fs::write(&tool, b"#!/bin/sh\n").expect("write tool");

        let emscripten_dir = temp.path().join("emscripten");
        fs::create_dir_all(&emscripten_dir).expect("create emscripten");
        let launcher = emscripten_dir.join("emcc");
        fs::write(&launcher, b"#!/bin/sh\n").expect("write launcher");

        for path in [&tool, &launcher] {
            let mut permissions = fs::metadata(path).expect("metadata").permissions();
            permissions.set_mode(0o644);
            fs::set_permissions(path, permissions).expect("clear executable bit");
        }

        ensure_toolchain_executables(temp.path()).expect("restore executable bits");

        for path in [&tool, &launcher] {
            let mode = fs::metadata(path).expect("metadata").permissions().mode();
            assert_ne!(mode & 0o111, 0);
        }
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
    fn installer_editor_json_preserves_custom_entries_and_is_idempotent() {
        let temp = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        let launch = temp.path().join("launch.json");
        let tasks = temp.path().join("tasks.json");
        let settings = temp.path().join("settings.json");
        fs::write(
            &launch,
            r#"{"custom":"λ","configurations":[null,{"name":"user"}]}"#,
        )
        .unwrap();
        fs::write(
            &tasks,
            r#"{"custom":true,"tasks":[{"label":"Run FastLED (Debug)","command":"user"},7]}"#,
        )
        .unwrap();
        fs::write(&settings, r#"{"custom":[1,2],"editor.tabSize":2}"#).unwrap();
        update_launch_json_at(&launch).unwrap();
        generate_fastled_tasks_at(&tasks).unwrap();
        update_repository_settings_at(&settings).unwrap();
        let parse = |path: &Path| match json::parse(&fs::read(path).unwrap()).unwrap() {
            JsonValue::Object(value) => value,
            _ => panic!("expected editor object"),
        };
        let launch_value = parse(&launch);
        assert_eq!(launch_value["custom"], JsonValue::String("λ".into()));
        let JsonValue::Array(configs) = &launch_value["configurations"] else {
            panic!("configs");
        };
        assert_eq!(configs.len(), 3);
        assert_eq!(
            configs[0],
            editor_templates().unwrap().remove("launch").unwrap()
        );
        assert_eq!(configs[1], JsonValue::Null);
        let task_value = parse(&tasks);
        let JsonValue::Array(task_entries) = &task_value["tasks"] else {
            panic!("tasks");
        };
        assert_eq!(task_entries.len(), 3);
        assert_eq!(
            task_entries[0],
            json::parse(br#"{"label":"Run FastLED (Debug)","command":"user"}"#).unwrap()
        );
        assert_eq!(task_entries[1], JsonValue::Signed(7));
        let JsonValue::Array(expected_tasks) = editor_templates().unwrap().remove("tasks").unwrap()
        else {
            panic!("template tasks");
        };
        assert_eq!(task_entries[2], expected_tasks[1]);
        let settings_value = parse(&settings);
        let JsonValue::Object(mut expected_settings) = fastled_repository_settings().unwrap()
        else {
            panic!("settings");
        };
        expected_settings.insert("custom".into(), json::parse(b"[1,2]").unwrap());
        assert_eq!(settings_value, expected_settings);
        let before = [&launch, &tasks, &settings].map(|path| fs::read(path).unwrap());
        update_launch_json_at(&launch).unwrap();
        generate_fastled_tasks_at(&tasks).unwrap();
        update_repository_settings_at(&settings).unwrap();
        assert_eq!(
            before,
            [&launch, &tasks, &settings].map(|path| fs::read(path).unwrap())
        );
        assert!(before.iter().all(|bytes| bytes.last() == Some(&b'\n')));
    }

    #[test]
    fn installer_editor_json_repairs_invalid_shapes_but_preserves_oversized_files() {
        let temp = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        let path = temp.path().join("editor.json");
        for invalid in ["{", "[]", "null"] {
            fs::write(&path, invalid).unwrap();
            update_launch_json_at(&path).unwrap();
            let JsonValue::Object(value) = json::parse(&fs::read(&path).unwrap()).unwrap() else {
                panic!("object");
            };
            assert_eq!(value["version"], JsonValue::String("0.2.0".into()));
        }
        fs::write(&path, r#"{"custom":1,"tasks":false}"#).unwrap();
        generate_fastled_tasks_at(&path).unwrap();
        let JsonValue::Object(value) = json::parse(&fs::read(&path).unwrap()).unwrap() else {
            panic!("object");
        };
        assert_eq!(value["custom"], JsonValue::Signed(1));
        assert!(matches!(&value["tasks"], JsonValue::Array(values) if values.len() == 2));
        let oversized = format!(r#"{{"custom":"{}"}}"#, "x".repeat(json::MAX_INPUT_BYTES));
        fs::write(&path, &oversized).unwrap();
        assert!(update_launch_json_at(&path).is_err());
        assert!(generate_fastled_tasks_at(&path).is_err());
        assert!(update_repository_settings_at(&path).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), oversized);
        let unpublished = temp.path().join("absent/settings.json");
        assert!(write_json_file(
            &unpublished,
            &JsonValue::String("x".repeat(json::MAX_OUTPUT_BYTES))
        )
        .is_err());
        assert!(!unpublished.parent().unwrap().exists());
    }

    #[test]
    fn manifest_positional_version_preserves_optional_trailing_parts() {
        for source in [
            r#"{"latest":"v","versions":{"v":["url","hash"]}}"#,
            r#"{"latest":"v","v":["url","hash"]}"#,
            r#"["v",{"v":["url","hash",null]}]"#,
        ] {
            let parsed = parse_platform_manifest(source).unwrap();
            let version = &parsed.versions["v"];
            assert_eq!(version.href, "url");
            assert_eq!(version.sha256, "hash");
            assert!(version.parts.is_none());
        }
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
