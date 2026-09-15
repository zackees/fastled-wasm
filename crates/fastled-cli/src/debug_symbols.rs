//! DWARF source-path resolution for the in-browser debugger.
//!
//! When a sketch is compiled in debug mode, emscripten embeds DWARF entries
//! that point at the original source files (sketch, FastLED library, emsdk
//! headers). The browser's devtools then issue `POST /dwarfsource` requests
//! to fetch the actual text by path. This module converts those request paths
//! into real on-disk locations, with traversal guards so that only files under
//! the configured roots are exposed.
//!
//! This is the Rust port of the legacy Python `debug_symbols.py` module that
//! used to back the Flask-based debug routes.

use std::path::{Path, PathBuf};

use crate::error_compat::{Context, Result};
use kernal_api::json::{self, Layout, Value};

pub const DEFAULT_FASTLED_PREFIX: &str = "fastledsource";
pub const DEFAULT_SKETCH_PREFIX: &str = "sketchsource";
pub const DEFAULT_DWARF_PREFIX: &str = "dwarfsource";
pub const DWARF_ROOTS_MANIFEST: &str = "dwarf-roots.json";

/// Configurable DWARF path prefixes loaded from
/// `<fastled-src>/platforms/wasm/compiler/build_flags.toml`.
#[derive(Debug, Clone)]
pub struct DwarfPrefixConfig {
    pub fastled_prefix: String,
    pub sketch_prefix: String,
    pub dwarf_prefix: String,
    pub file_prefix_map_from: Option<String>,
    pub file_prefix_map_to: Option<String>,
}

fn default_fastled_prefix() -> String {
    DEFAULT_FASTLED_PREFIX.to_string()
}

fn default_sketch_prefix() -> String {
    DEFAULT_SKETCH_PREFIX.to_string()
}

fn default_dwarf_prefix() -> String {
    DEFAULT_DWARF_PREFIX.to_string()
}

impl Default for DwarfPrefixConfig {
    fn default() -> Self {
        Self {
            fastled_prefix: default_fastled_prefix(),
            sketch_prefix: default_sketch_prefix(),
            dwarf_prefix: default_dwarf_prefix(),
            file_prefix_map_from: None,
            file_prefix_map_to: None,
        }
    }
}

impl DwarfPrefixConfig {
    pub(crate) fn from_config(value: &kernal_api::config::Value) -> Result<Self> {
        use kernal_api::config::Value;
        let Value::Table(fields) = value else {
            crate::error_compat::bail!("dwarf must be a table");
        };
        let string = |name: &str| -> Result<Option<String>> {
            match fields.get(name) {
                None => Ok(None),
                Some(Value::String(value)) => Ok(Some(value.clone())),
                Some(_) => crate::error_compat::bail!("dwarf.{name} must be a string"),
            }
        };
        Ok(Self {
            fastled_prefix: string("fastled_prefix")?.unwrap_or_else(default_fastled_prefix),
            sketch_prefix: string("sketch_prefix")?.unwrap_or_else(default_sketch_prefix),
            dwarf_prefix: string("dwarf_prefix")?.unwrap_or_else(default_dwarf_prefix),
            file_prefix_map_from: string("file_prefix_map_from")?,
            file_prefix_map_to: string("file_prefix_map_to")?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct DebugSymbolConfig {
    pub sketch_dir: PathBuf,
    pub fastled_dir: Option<PathBuf>,
    pub emsdk_path: Option<PathBuf>,
    pub prefixes: DwarfPrefixConfig,
}

impl DebugSymbolConfig {
    /// The set of named source roots that paths can resolve into.
    ///
    /// Order matters: longer / more-specific prefixes should win, but every
    /// configured prefix is matched literally so ordering only matters for
    /// equal-prefix ambiguities (none currently).
    pub fn source_roots(&self) -> Vec<(String, PathBuf)> {
        let mut roots: Vec<(String, PathBuf)> = Vec::new();
        let sketch = canonicalize_or(&self.sketch_dir);
        roots.push((self.prefixes.sketch_prefix.clone(), sketch));
        if let Some(fastled_dir) = &self.fastled_dir {
            let src = canonicalize_or(&fastled_dir.join("src"));
            roots.push((self.prefixes.fastled_prefix.clone(), src.clone()));
            // `headers` matches what the legacy resolver exposed so that DWARF
            // entries that point at `headers/...` (resolved relative to the
            // FastLED include root) keep working.
            roots.push(("headers".to_string(), src));
        }
        if let Some(emsdk_path) = &self.emsdk_path {
            roots.push(("emsdk".to_string(), canonicalize_or(emsdk_path)));
        }
        roots
    }
}

fn canonicalize_or(path: &Path) -> PathBuf {
    crate::path::canonicalize_normalized(path).into_path_buf()
}

/// Load DWARF prefix overrides from
/// `<fastled-src>/platforms/wasm/compiler/build_flags.toml` if present.
///
/// Falls back to defaults when the file or the `[dwarf]` table is absent so
/// callers can rely on this for unconditional setup.
pub fn load_debug_symbol_config(
    sketch_dir: PathBuf,
    fastled_dir: Option<PathBuf>,
    emsdk_path: Option<PathBuf>,
) -> DebugSymbolConfig {
    let prefixes = fastled_dir
        .as_ref()
        .and_then(|root| read_dwarf_prefixes(root))
        .unwrap_or_default();

    DebugSymbolConfig {
        sketch_dir,
        fastled_dir,
        emsdk_path,
        prefixes,
    }
}

fn read_dwarf_prefixes(fastled_dir: &Path) -> Option<DwarfPrefixConfig> {
    let candidate = fastled_dir
        .join("src")
        .join("platforms")
        .join("wasm")
        .join("compiler")
        .join("build_flags.toml");
    let text = std::fs::read_to_string(&candidate).ok()?;
    let parsed = kernal_api::config::Document::parse_toml(&text).ok()?;
    let kernal_api::config::Value::Table(fields) = parsed.root() else {
        return None;
    };
    DwarfPrefixConfig::from_config(fields.get("dwarf")?).ok()
}

// These record layouts and duplicate-field rules belong to the manifest
// schema. JSON syntax, resource bounds and member preservation live upstream.
fn manifest_fields<const N: usize>(
    value: Value,
    names: [&str; N],
    minimum_sequence_len: usize,
) -> Result<[Option<Value>; N]> {
    let mut fields = std::array::from_fn(|_| None);
    match value {
        Value::ObjectMembers(members) => {
            for (name, value) in members {
                if let Some(index) = names.iter().position(|field| *field == name) {
                    if fields[index].replace(value).is_some() {
                        crate::error_compat::bail!(
                            "duplicate debug manifest field {}",
                            names[index]
                        );
                    }
                }
            }
        }
        Value::Array(values) if (minimum_sequence_len..=N).contains(&values.len()) => {
            for (field, value) in fields.iter_mut().zip(values) {
                *field = Some(value);
            }
        }
        _ => crate::error_compat::bail!("invalid debug manifest record"),
    }
    Ok(fields)
}

fn manifest_string(value: Option<Value>, name: &str, default: Option<&str>) -> Result<String> {
    match (value, default) {
        (Some(Value::String(value)), _) => Ok(value),
        (None, Some(default)) => Ok(default.to_owned()),
        _ => crate::error_compat::bail!("debug manifest {name} must be a string"),
    }
}

fn manifest_optional_string(value: Option<Value>, name: &str) -> Result<Option<String>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        value => manifest_string(value, name, None).map(Some),
    }
}

fn manifest_config(value: Value) -> Result<DebugSymbolConfig> {
    let [sketch, fastled, emsdk, prefixes] = manifest_fields(
        value,
        ["sketch_dir", "fastled_dir", "emsdk_path", "prefixes"],
        4,
    )?;
    let [fastled_prefix, sketch_prefix, dwarf_prefix, map_from, map_to] = manifest_fields(
        prefixes.context("missing debug manifest prefixes")?,
        [
            "fastled_prefix",
            "sketch_prefix",
            "dwarf_prefix",
            "file_prefix_map_from",
            "file_prefix_map_to",
        ],
        0,
    )?;
    Ok(DebugSymbolConfig {
        sketch_dir: manifest_string(sketch, "sketch_dir", None)?.into(),
        fastled_dir: manifest_optional_string(fastled, "fastled_dir")?.map(PathBuf::from),
        emsdk_path: manifest_optional_string(emsdk, "emsdk_path")?.map(PathBuf::from),
        prefixes: DwarfPrefixConfig {
            fastled_prefix: manifest_string(
                fastled_prefix,
                "fastled_prefix",
                Some(DEFAULT_FASTLED_PREFIX),
            )?,
            sketch_prefix: manifest_string(
                sketch_prefix,
                "sketch_prefix",
                Some(DEFAULT_SKETCH_PREFIX),
            )?,
            dwarf_prefix: manifest_string(
                dwarf_prefix,
                "dwarf_prefix",
                Some(DEFAULT_DWARF_PREFIX),
            )?,
            file_prefix_map_from: manifest_optional_string(map_from, "file_prefix_map_from")?,
            file_prefix_map_to: manifest_optional_string(map_to, "file_prefix_map_to")?,
        },
    })
}

fn manifest_document(config: &DebugSymbolConfig) -> Result<Value> {
    let path = |path: &Path| -> Result<Value> {
        Ok(Value::String(
            path.to_str()
                .context("debug manifest path must be UTF-8")?
                .to_owned(),
        ))
    };
    let mut prefixes = vec![
        (
            "fastled_prefix".into(),
            Value::String(config.prefixes.fastled_prefix.clone()),
        ),
        (
            "sketch_prefix".into(),
            Value::String(config.prefixes.sketch_prefix.clone()),
        ),
        (
            "dwarf_prefix".into(),
            Value::String(config.prefixes.dwarf_prefix.clone()),
        ),
    ];
    for (name, value) in [
        (
            "file_prefix_map_from",
            &config.prefixes.file_prefix_map_from,
        ),
        ("file_prefix_map_to", &config.prefixes.file_prefix_map_to),
    ] {
        if let Some(value) = value {
            prefixes.push((name.into(), Value::String(value.clone())));
        }
    }
    Ok(Value::ObjectMembers(vec![
        ("version".into(), Value::Unsigned(1)),
        (
            "config".into(),
            Value::ObjectMembers(vec![
                ("sketch_dir".into(), path(&config.sketch_dir)?),
                (
                    "fastled_dir".into(),
                    config
                        .fastled_dir
                        .as_deref()
                        .map(path)
                        .transpose()?
                        .unwrap_or(Value::Null),
                ),
                (
                    "emsdk_path".into(),
                    config
                        .emsdk_path
                        .as_deref()
                        .map(path)
                        .transpose()?
                        .unwrap_or(Value::Null),
                ),
                ("prefixes".into(), Value::ObjectMembers(prefixes)),
            ]),
        ),
    ]))
}

pub fn write_debug_symbol_manifest(
    output_dir: &Path,
    config: &DebugSymbolConfig,
) -> Result<PathBuf> {
    std::fs::create_dir_all(output_dir)
        .with_context(|| format!("create {}", output_dir.display()))?;
    let path = output_dir.join(DWARF_ROOTS_MANIFEST);
    let json = json::encode(&manifest_document(config)?, Layout::Pretty)?;
    std::fs::write(&path, json).with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}

pub fn read_debug_symbol_manifest(output_dir: &Path) -> Result<Option<DebugSymbolConfig>> {
    let path = output_dir.join(DWARF_ROOTS_MANIFEST);
    if !path.is_file() {
        return Ok(None);
    }
    let json =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let parse = || -> Result<(u32, DebugSymbolConfig)> {
        let [version, config] = manifest_fields(
            json::parse_members(json.as_bytes())?,
            ["version", "config"],
            2,
        )?;
        let version = match version {
            Some(Value::Signed(value)) => u32::try_from(value)?,
            Some(Value::Unsigned(value)) => u32::try_from(value)?,
            _ => crate::error_compat::bail!("debug manifest version must be an unsigned integer"),
        };
        Ok((
            version,
            manifest_config(config.context("missing debug manifest config")?)?,
        ))
    };
    let (version, config) = parse().with_context(|| format!("parse {}", path.display()))?;
    if version != 1 {
        crate::error_compat::bail!(
            "unsupported debug symbol manifest version {} in {}",
            version,
            path.display()
        );
    }
    Ok(Some(config))
}

/// Resolves browser-issued DWARF paths to absolute file paths on disk.
#[derive(Debug, Clone)]
pub struct DebugSymbolResolver {
    config: DebugSymbolConfig,
}

impl DebugSymbolResolver {
    pub fn new(config: DebugSymbolConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &DebugSymbolConfig {
        &self.config
    }

    fn known_prefixes(&self) -> Vec<String> {
        let mut prefixes = vec![
            self.config.prefixes.fastled_prefix.clone(),
            self.config.prefixes.sketch_prefix.clone(),
            self.config.prefixes.dwarf_prefix.clone(),
        ];
        for (prefix, _) in self.config.source_roots() {
            if !prefixes.iter().any(|known| known == &prefix) {
                prefixes.push(prefix);
            }
        }
        prefixes
    }

    /// Strip leading garbage from a DWARF-issued path so that what remains
    /// starts with one of the configured prefixes (e.g.
    /// `/build/fastledsource/FastLED.h` → `fastledsource/FastLED.h`).
    /// Returns `None` if no prefix is present.
    pub fn prune_path(&self, request_path: &str) -> Option<String> {
        let normalized = normalize_input_path(request_path);
        let prefixes = self.known_prefixes();

        for prefix in &prefixes {
            if normalized == *prefix {
                return Some(normalized);
            }
            let with_slash = format!("{prefix}/");
            if normalized.starts_with(&with_slash) {
                return Some(normalized);
            }
        }

        let parts: Vec<&str> = normalized.split('/').filter(|p| !p.is_empty()).collect();
        let mut prefix_index: Option<usize> = None;
        for (idx, part) in parts.iter().enumerate() {
            if prefixes.iter().any(|prefix| prefix == part) {
                prefix_index = Some(idx);
            }
        }
        let idx = prefix_index?;
        let result = parts[idx..].join("/");
        Some(result)
    }

    /// Map a DWARF request path to a real file. Errors:
    /// - `Invalid`: traversal attempts, missing prefix, mismatched root
    /// - `NotFound`: prefix and root matched but the file doesn't exist
    pub fn resolve(&self, request_path: &str, check_exists: bool) -> Result<PathBuf, ResolveError> {
        if request_path.split(['/', '\\']).any(|seg| seg == "..") {
            return Err(ResolveError::Invalid(format!(
                "invalid path: {request_path}"
            )));
        }

        let pruned = self
            .prune_path(request_path)
            .ok_or_else(|| ResolveError::Invalid(format!("invalid path: {request_path}")))?;

        let mut normalized = pruned.replace('\\', "/");
        normalized = normalized.trim_start_matches('/').to_string();

        // `dwarfsource` is a sibling-prefix that wraps the named-root form,
        // e.g. `dwarfsource/emsdk/upstream/.../cache.h`. Strip the outer
        // prefix and let the rest match the actual root.
        let dwarf_prefix_with_slash = format!("{}/", self.config.prefixes.dwarf_prefix);
        if normalized.starts_with(&dwarf_prefix_with_slash) {
            normalized = normalized[dwarf_prefix_with_slash.len()..].to_string();
        } else if normalized == self.config.prefixes.dwarf_prefix {
            return Err(ResolveError::Invalid(format!(
                "invalid path: {request_path}"
            )));
        }

        for (prefix, root) in self.config.source_roots() {
            let target = if normalized == prefix {
                root.clone()
            } else {
                let with_slash = format!("{prefix}/");
                if let Some(suffix) = normalized.strip_prefix(&with_slash) {
                    root.join(suffix)
                } else {
                    continue;
                }
            };

            return finalize_target(&target, &root, check_exists, request_path);
        }

        // Last-ditch fallback: try the path as if it were already relative to
        // FastLED's include root.
        if let Some(fastled_dir) = &self.config.fastled_dir {
            let fastled_root = canonicalize_or(&fastled_dir.join("src"));
            let target = fastled_root.join(&normalized);
            if let Ok(resolved) =
                finalize_target(&target, &fastled_root, check_exists, request_path)
            {
                return Ok(resolved);
            }
        }

        Err(ResolveError::Invalid(format!(
            "invalid path: {request_path}"
        )))
    }
}

fn finalize_target(
    target: &Path,
    root: &Path,
    check_exists: bool,
    request_path: &str,
) -> Result<PathBuf, ResolveError> {
    let resolved_target = lexically_normalize(target);
    let resolved_root = lexically_normalize(root);
    if !is_within(&resolved_target, &resolved_root) {
        return Err(ResolveError::Invalid(format!(
            "invalid path: {request_path}"
        )));
    }
    if check_exists && !resolved_target.exists() {
        return Err(ResolveError::NotFound(format!(
            "could not find path {}",
            resolved_target.display()
        )));
    }
    Ok(resolved_target)
}

fn lexically_normalize(path: &Path) -> PathBuf {
    crate::path::canonicalize_normalized(path).into_path_buf()
}

fn is_within(path: &Path, root: &Path) -> bool {
    path.starts_with(root)
}

fn normalize_input_path(request_path: &str) -> String {
    request_path
        .trim()
        .replace('\\', "/")
        .trim_start_matches('/')
        .to_string()
}

#[derive(Debug)]
pub enum ResolveError {
    Invalid(String),
    NotFound(String),
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::Invalid(msg) | ResolveError::NotFound(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for ResolveError {}

/// Best-effort guess at the emscripten install root, used to expose the
/// `emsdk` source prefix when DWARF entries point at emsdk headers.
pub fn guess_emsdk_path() -> Option<PathBuf> {
    if let Some(emsdk) = std::env::var_os("EMSDK") {
        return Some(PathBuf::from(emsdk));
    }
    if let Some(install_dir) = std::env::var_os("FASTLED_EMSCRIPTEN_DIR").map(PathBuf::from) {
        return Some(install_dir);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernal_api::platform::fs::TemporaryDirectory;
    use std::fs;

    #[test]
    fn manifest_json_schema_preserves_defaults_and_nested_duplicates() {
        let tmp = TemporaryDirectory::new().unwrap();
        let path = tmp.path().join(DWARF_ROOTS_MANIFEST);
        for source in [
            r#"{"version":1,"config":{"sketch_dir":"日本","prefixes":{}}}"#,
            r#"{"version":1,"config":{"sketch_dir":"日本","fastled_dir":null,"prefixes":{"future":1,"future":2}}}"#,
            r#"[1,["日本",null,null,[]]]"#,
        ] {
            fs::write(&path, source).unwrap();
            let config = read_debug_symbol_manifest(tmp.path()).unwrap().unwrap();
            assert_eq!(config.sketch_dir, PathBuf::from("日本"));
            assert_eq!(config.fastled_dir, None);
            assert_eq!(config.emsdk_path, None);
            assert_eq!(config.prefixes.sketch_prefix, DEFAULT_SKETCH_PREFIX);
            assert_eq!(config.prefixes.file_prefix_map_from, None);
            write_debug_symbol_manifest(tmp.path(), &config).unwrap();
            let output = fs::read_to_string(&path).unwrap();
            assert!(output.contains("\"fastled_dir\": null"));
            assert!(!output.contains("file_prefix_map_from"));
            assert!(!output.ends_with('\n'));
        }
        for source in [
            r#"{"version":2,"config":{"sketch_dir":"a","prefixes":{}}}"#,
            r#"{"version":1.0,"config":{"sketch_dir":"a","prefixes":{}}}"#,
            r#"{"version":1,"version":1,"config":{"sketch_dir":"a","prefixes":{}}}"#,
            r#"{"version":1,"config":{"sketch_dir":"a","fastled_dir":null,"fastled_dir":null,"prefixes":{}}}"#,
            r#"{"version":1,"config":{"sketch_dir":"a","prefixes":{"sketch_prefix":"a","sketch_prefix":"b"}}}"#,
            r#"{"version":1,"config":{"sketch_dir":"a","prefixes":{"sketch_prefix":null}}}"#,
            r#"{"version":1,"config":{"sketch_dir":"a"}}"#,
            r#"[1,["a"]]"#,
        ] {
            fs::write(&path, source).unwrap();
            assert!(
                read_debug_symbol_manifest(tmp.path()).is_err(),
                "accepted {source}"
            );
        }
    }

    #[test]
    fn dwarf_schema_ignores_unrelated_sections_and_defaults_invalid_overrides() {
        let tmp = TemporaryDirectory::new().unwrap();
        let compiler = tmp.path().join("src/platforms/wasm/compiler");
        fs::create_dir_all(&compiler).unwrap();
        let path = compiler.join("build_flags.toml");
        // This reader validates only DWARF policy, not compiler flag schemas.
        fs::write(&path, "all = 1\n[dwarf]\nsketch_prefix = 'custom'\n").unwrap();
        let parsed = read_dwarf_prefixes(tmp.path()).unwrap();
        assert_eq!(parsed.sketch_prefix, "custom");
        assert_eq!(parsed.fastled_prefix, DEFAULT_FASTLED_PREFIX);
        for source in ["[dwarf]\nsketch_prefix = 1", "x =", "all = 1"] {
            fs::write(&path, source).unwrap();
            assert!(
                read_dwarf_prefixes(tmp.path()).is_none(),
                "accepted {source}"
            );
            let config = load_debug_symbol_config(
                tmp.path().join("sketch"),
                Some(tmp.path().to_path_buf()),
                None,
            );
            assert_eq!(config.prefixes.sketch_prefix, DEFAULT_SKETCH_PREFIX);
        }
    }

    #[cfg(unix)]
    #[test]
    fn manifest_json_rejects_non_utf8_paths_before_replacing_file() {
        use std::os::unix::ffi::OsStringExt;
        let tmp = TemporaryDirectory::new().unwrap();
        let path = tmp.path().join(DWARF_ROOTS_MANIFEST);
        fs::write(&path, "original").unwrap();
        let config = load_debug_symbol_config(
            PathBuf::from(std::ffi::OsString::from_vec(vec![0xff])),
            None,
            None,
        );
        assert!(write_debug_symbol_manifest(tmp.path(), &config).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "original");
    }

    fn setup_dirs() -> (TemporaryDirectory, PathBuf, PathBuf, PathBuf) {
        let tmp = TemporaryDirectory::new().unwrap();
        let sketch_dir = tmp.path().join("sketch");
        let fastled_dir = tmp.path().join("FastLED");
        let emsdk_dir = tmp.path().join("emsdk");

        fs::create_dir_all(sketch_dir.join("src")).unwrap();
        fs::create_dir_all(fastled_dir.join("src")).unwrap();
        fs::create_dir_all(emsdk_dir.join("upstream").join("emscripten")).unwrap();

        fs::write(sketch_dir.join("src").join("demo.ino"), "sketch").unwrap();
        fs::write(fastled_dir.join("src").join("FastLED.h"), "fastled").unwrap();
        fs::write(
            emsdk_dir
                .join("upstream")
                .join("emscripten")
                .join("cache.h"),
            "emsdk",
        )
        .unwrap();

        (tmp, sketch_dir, fastled_dir, emsdk_dir)
    }

    #[test]
    fn resolves_sketch_fastled_and_emsdk_prefixes() {
        let (_tmp, sketch_dir, fastled_dir, emsdk_dir) = setup_dirs();
        let resolver = DebugSymbolResolver::new(load_debug_symbol_config(
            sketch_dir.clone(),
            Some(fastled_dir.clone()),
            Some(emsdk_dir.clone()),
        ));

        let sketch = resolver.resolve("sketchsource/src/demo.ino", true).unwrap();
        assert!(sketch.ends_with("demo.ino"));

        let fastled = resolver.resolve("fastledsource/FastLED.h", true).unwrap();
        assert!(fastled.ends_with("FastLED.h"));

        let emsdk = resolver
            .resolve("dwarfsource/emsdk/upstream/emscripten/cache.h", true)
            .unwrap();
        assert!(emsdk.ends_with("cache.h"));
    }

    #[test]
    fn rejects_traversal_attempts() {
        let (_tmp, sketch_dir, _, _) = setup_dirs();
        let resolver = DebugSymbolResolver::new(load_debug_symbol_config(sketch_dir, None, None));
        let err = resolver
            .resolve("dwarfsource/../../secret.txt", true)
            .unwrap_err();
        match err {
            ResolveError::Invalid(_) => {}
            _ => panic!("expected Invalid"),
        }
    }

    #[test]
    fn missing_file_returns_not_found() {
        let (_tmp, sketch_dir, _, _) = setup_dirs();
        let resolver = DebugSymbolResolver::new(load_debug_symbol_config(sketch_dir, None, None));
        let err = resolver
            .resolve("sketchsource/missing.cpp", true)
            .unwrap_err();
        match err {
            ResolveError::NotFound(_) => {}
            _ => panic!("expected NotFound"),
        }
    }

    #[test]
    fn prune_strips_leading_directories_before_prefix() {
        let (_tmp, sketch_dir, fastled_dir, _) = setup_dirs();
        let resolver = DebugSymbolResolver::new(load_debug_symbol_config(
            sketch_dir,
            Some(fastled_dir),
            None,
        ));
        let pruned = resolver
            .prune_path("/build/wasm/fastledsource/FastLED.h")
            .unwrap();
        assert_eq!(pruned, "fastledsource/FastLED.h");
    }

    #[test]
    fn invalid_path_with_no_prefix_returns_invalid() {
        let (_tmp, sketch_dir, _, _) = setup_dirs();
        let resolver = DebugSymbolResolver::new(load_debug_symbol_config(sketch_dir, None, None));
        let err = resolver.resolve("nothing/to/see/here.h", true).unwrap_err();
        match err {
            ResolveError::Invalid(_) => {}
            _ => panic!("expected Invalid"),
        }
    }

    #[test]
    fn build_flags_toml_overrides_prefixes() {
        let (_tmp, sketch_dir, fastled_dir, _) = setup_dirs();
        let compiler_dir = fastled_dir.join("src/platforms/wasm/compiler");
        fs::create_dir_all(&compiler_dir).unwrap();
        fs::write(
            compiler_dir.join("build_flags.toml"),
            r#"
[dwarf]
fastled_prefix = "fl_src"
sketch_prefix = "fl_sketch"
dwarf_prefix = "fl_dwarf"
"#,
        )
        .unwrap();

        let config = load_debug_symbol_config(sketch_dir, Some(fastled_dir), None);
        assert_eq!(config.prefixes.fastled_prefix, "fl_src");
        assert_eq!(config.prefixes.sketch_prefix, "fl_sketch");
        assert_eq!(config.prefixes.dwarf_prefix, "fl_dwarf");
    }

    #[test]
    fn debug_symbol_manifest_round_trips_config() {
        let (tmp, sketch_dir, fastled_dir, emsdk_dir) = setup_dirs();
        let output_dir = tmp.path().join("fastled_js");
        let config = load_debug_symbol_config(
            sketch_dir.clone(),
            Some(fastled_dir.clone()),
            Some(emsdk_dir.clone()),
        );

        let path = write_debug_symbol_manifest(&output_dir, &config).unwrap();
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some(DWARF_ROOTS_MANIFEST)
        );

        let loaded = read_debug_symbol_manifest(&output_dir)
            .unwrap()
            .expect("manifest should exist");
        let resolver = DebugSymbolResolver::new(loaded);
        let resolved = resolver.resolve("sketchsource/src/demo.ino", true).unwrap();
        let expected =
            crate::path::canonicalize_normalized(&sketch_dir.join("src").join("demo.ino"))
                .into_path_buf();
        assert_eq!(resolved, expected);
    }

    #[test]
    fn direct_emsdk_prefix_resolves_from_source_roots() {
        let (_tmp, sketch_dir, fastled_dir, emsdk_dir) = setup_dirs();
        let resolver = DebugSymbolResolver::new(load_debug_symbol_config(
            sketch_dir,
            Some(fastled_dir),
            Some(emsdk_dir),
        ));

        let emsdk = resolver
            .resolve("emsdk/upstream/emscripten/cache.h", true)
            .unwrap();
        assert!(emsdk.ends_with("cache.h"));
    }

    #[cfg(windows)]
    #[test]
    fn source_roots_strip_windows_long_path_prefix() {
        let config = DebugSymbolConfig {
            sketch_dir: PathBuf::from(r"\\?\C:\tmp\sketch"),
            fastled_dir: None,
            emsdk_path: None,
            prefixes: DwarfPrefixConfig::default(),
        };

        let roots = config.source_roots();
        assert!(!roots[0].1.display().to_string().starts_with(r"\\?\"));
    }
}
