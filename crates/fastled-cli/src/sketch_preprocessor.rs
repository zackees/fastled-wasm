//! Arduino `.ino` preprocessing shared by the WASM build and editor support.
//!
//! Product integration retains FastLED/fbuild source-scanner provenance at
//! `1e75ccf5a4ca922b4d922a6da286b965fac8832d` (see #206). Generic C++ analysis
//! now lives in kernal-api; Arduino selection and editor publication stay here.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::{self, Read};
use std::path::Path;

use anyhow::{bail, Context, Result};
use kernal_api::hash::Sha256Hasher as Sha256;
use serde::{Deserialize, Serialize};

use crate::path::NormalizedPath;

/// A VS Code document buffer. Paths must name top-level `.ino` tabs in the
/// sketch directory; unopened tabs are loaded from disk.
#[derive(Debug, Clone, Deserialize)]
pub struct SnapshotDocument {
    pub path: String,
    pub version: i64,
    pub text: String,
}

/// JSON protocol sent over stdin by the VS Code extension.
#[derive(Debug, Deserialize)]
pub struct SnapshotRequest {
    pub sketch_dir: String,
    pub generation: u64,
    #[serde(default)]
    pub documents: Vec<SnapshotDocument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestDocument {
    pub path: String,
    pub version: Option<i64>,
    pub sha256: String,
}

/// Completion marker for an IntelliSense cache generation. Consumers should
/// only use the source/header named by this manifest after validating hashes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotManifest {
    pub schema: u32,
    pub generation: u64,
    pub documents: Vec<ManifestDocument>,
    pub generated_source: String,
    pub generated_source_sha256: String,
    pub prototype_header: String,
    pub prototype_header_sha256: String,
}

#[derive(Debug, Clone)]
pub struct PreprocessedSketch {
    pub translation_unit: String,
    pub prototype_header: String,
    pub documents: Vec<ManifestDocument>,
}

/// Read the extension request, render a canonical source view, and publish it
/// under `<sketch>/.fastled/intellisense`. A malformed half-typed source is
/// never published, so the previous manifest/header remain available.
pub fn run_stdin_snapshot() -> Result<()> {
    let mut request_json = String::new();
    io::stdin()
        .read_to_string(&mut request_json)
        .context("read IntelliSense snapshot request from stdin")?;
    let request: SnapshotRequest =
        serde_json::from_str(&request_json).context("parse IntelliSense snapshot JSON")?;
    let manifest = write_live_snapshot(&request)?;
    println!("{}", serde_json::to_string(&manifest)?);
    Ok(())
}

pub fn preprocess_disk(sketch_dir: &Path) -> Result<PreprocessedSketch> {
    preprocess(sketch_dir, &BTreeMap::new())
}

pub fn write_disk_snapshot(sketch_dir: &Path) -> Result<(SnapshotManifest, NormalizedPath)> {
    let request = SnapshotRequest {
        sketch_dir: sketch_dir.to_string_lossy().into_owned(),
        generation: 0,
        documents: Vec::new(),
    };
    // A tab topology refresh must replace a previous live generation so the
    // compile database reflects renamed/deleted disk tabs. The extension
    // immediately follows this with a newer live snapshot when buffers exist.
    let manifest = write_snapshot(&request, true)?;
    Ok((
        manifest,
        intellisense_dir(sketch_dir).join("prototypes.hpp"),
    ))
}

pub fn write_live_snapshot(request: &SnapshotRequest) -> Result<SnapshotManifest> {
    write_snapshot(request, false)
}

fn write_snapshot(request: &SnapshotRequest, force_disk_refresh: bool) -> Result<SnapshotManifest> {
    let sketch_dir = crate::path::canonicalize_normalized(Path::new(&request.sketch_dir));
    if !sketch_dir.is_dir() {
        bail!("sketch directory does not exist: {}", sketch_dir.display());
    }

    let mut open_documents: BTreeMap<NormalizedPath, SnapshotDocument> = BTreeMap::new();
    for document in &request.documents {
        let path = crate::path::canonicalize_normalized(Path::new(&document.path));
        if path.as_path().parent() != Some(sketch_dir.as_path()) || !is_ino(&path) {
            bail!(
                "snapshot document must be a top-level .ino tab in {}: {}",
                sketch_dir.display(),
                path.display()
            );
        }
        open_documents.insert(path, document.clone());
    }

    let cache_dir = intellisense_dir(&sketch_dir);
    fs::create_dir_all(
        cache_dir
            .as_path()
            .parent()
            .expect("intellisense cache parent"),
    )?;
    fs::write(
        cache_dir
            .as_path()
            .parent()
            .expect("cache parent")
            .join(".gitignore"),
        "*\n!.gitignore\n",
    )?;
    let lock_path = cache_dir
        .as_path()
        .parent()
        .expect("cache parent")
        .join(".lock");
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .with_context(|| format!("open IntelliSense lock {}", lock_path.display()))?;
    let _guard =
        kernal_api::platform::fs::lock_exclusive(&lock).context("lock IntelliSense snapshot")?;

    if let Some(existing) = read_manifest(&cache_dir) {
        if !force_disk_refresh && existing.generation > request.generation {
            return Ok(existing);
        }
    }

    // Do all parsing before writing anything. This preserves the last good
    // prelude when VS Code sends a transient syntactically incomplete buffer.
    let rendered = preprocess(sketch_dir.as_path(), &open_documents)?;
    let manifest = SnapshotManifest {
        schema: 1,
        generation: request.generation,
        documents: rendered.documents,
        generated_source: "sketch.cpp".to_string(),
        generated_source_sha256: sha256(&rendered.translation_unit),
        prototype_header: "prototypes.hpp".to_string(),
        prototype_header_sha256: sha256(&rendered.prototype_header),
    };

    fs::create_dir_all(&cache_dir)?;
    atomic_write(
        &cache_dir.join(&manifest.generated_source),
        &rendered.translation_unit,
    )?;
    atomic_write(
        &cache_dir.join(&manifest.prototype_header),
        &rendered.prototype_header,
    )?;
    // The manifest is written last and is the atomic publication marker.
    atomic_write(
        &cache_dir.join("manifest.json"),
        &serde_json::to_string_pretty(&manifest)?,
    )?;
    Ok(manifest)
}

fn preprocess(
    sketch_dir: &Path,
    open_documents: &BTreeMap<NormalizedPath, SnapshotDocument>,
) -> Result<PreprocessedSketch> {
    let tabs = discover_tabs(sketch_dir)?;
    let mut contents = Vec::with_capacity(tabs.len());
    let mut documents = Vec::with_capacity(tabs.len());
    for tab in &tabs {
        let (text, version) = if let Some(document) = open_documents.get(tab) {
            (document.text.clone(), Some(document.version))
        } else {
            (
                fs::read_to_string(tab).with_context(|| format!("read {}", tab.display()))?,
                None,
            )
        };
        let text = normalize_line_endings(&text);
        documents.push(ManifestDocument {
            path: display_path(tab),
            version,
            sha256: sha256(&text),
        });
        contents.push((tab.clone(), text));
    }

    let combined = contents
        .iter()
        .map(|(_, text)| text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let prototypes = extract_function_prototypes(&combined)?;
    let prototype_declarations = render_prototype_declarations(&prototypes);
    let prototype_header = format!(
        "#pragma once\n// Auto-generated Arduino sketch prototypes.\n{prototype_declarations}"
    );

    let mut translation_unit = String::from(
        "// Generated by fastled sketch preprocessor; do not edit.\n\
         // fbuild provenance: 1e75ccf5a4ca922b4d922a6da286b965fac8832d\n\
         import \"wasm_pch.h\";\n\n",
    );
    translation_unit.push_str(&prototype_declarations);
    translation_unit.push('\n');
    for (tab, content) in contents {
        translation_unit.push_str(&format!("#line 1 \"{}\"\n", display_path(&tab)));
        translation_unit.push_str(&content);
        if !content.ends_with('\n') {
            translation_unit.push('\n');
        }
    }

    Ok(PreprocessedSketch {
        translation_unit,
        prototype_header,
        documents,
    })
}

fn discover_tabs(sketch_dir: &Path) -> Result<Vec<NormalizedPath>> {
    let mut tabs = fs::read_dir(sketch_dir)
        .with_context(|| format!("read sketch directory {}", sketch_dir.display()))?
        .filter_map(|entry| entry.ok().map(|entry| NormalizedPath::new(entry.path())))
        .filter(|path| path.is_file() && is_ino(path))
        .collect::<Vec<_>>();
    if tabs.is_empty() {
        bail!("sketch has no .ino files: {}", sketch_dir.display());
    }
    tabs.sort_by(|a, b| {
        a.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase()
            .cmp(
                &b.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_ascii_lowercase(),
            )
    });
    let primary = sketch_dir.file_name().and_then(|name| name.to_str());
    if let Some(index) = primary.and_then(|name| {
        tabs.iter().position(|tab| {
            tab.file_stem()
                .and_then(|stem| stem.to_str())
                .is_some_and(|stem| stem.eq_ignore_ascii_case(name))
        })
    }) {
        let primary = tabs.remove(index);
        tabs.insert(0, primary);
    }
    Ok(tabs)
}

fn is_ino(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("ino"))
}

fn intellisense_dir(sketch_dir: &Path) -> NormalizedPath {
    NormalizedPath::new(sketch_dir)
        .join(".fastled")
        .join("intellisense")
}

fn read_manifest(cache_dir: &Path) -> Option<SnapshotManifest> {
    serde_json::from_str(&fs::read_to_string(cache_dir.join("manifest.json")).ok()?).ok()
}

fn atomic_write(path: &Path, contents: &str) -> Result<()> {
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(&temporary, contents).with_context(|| format!("write {}", temporary.display()))?;
    if path.exists() {
        // On Windows, rename cannot replace an existing file. The manifest is
        // committed last, so a reader will never accept a mixed generation.
        fs::remove_file(path).with_context(|| format!("replace {}", path.display()))?;
    }
    fs::rename(&temporary, path).with_context(|| format!("publish {}", path.display()))?;
    Ok(())
}

fn display_path(path: &Path) -> String {
    crate::path::canonicalize_normalized(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn sha256(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn normalize_line_endings(source: &str) -> String {
    source.replace("\r\n", "\n").replace('\r', "\n")
}

fn render_prototype_declarations(prototypes: &[String]) -> String {
    let mut declarations = String::new();
    for prototype in prototypes {
        declarations.push_str(prototype);
        declarations.push_str(";\n");
    }
    declarations
}

/// Select Arduino prototypes from kernel-owned C++ syntax analysis.
fn extract_function_prototypes(source: &str) -> Result<Vec<String>> {
    use kernal_api::source::{analyze_cpp, AnalysisError};
    let candidates = match analyze_cpp(source) {
        Err(AnalysisError::InvalidSyntax) => {
            bail!("sketch has incomplete C++ syntax; retaining the last good IntelliSense prelude");
        }
        result => result.context("analyze Arduino sketch")?,
    };
    let mut seen = HashSet::new();
    let mut prototypes = Vec::new();
    for candidate in candidates {
        let context = candidate.context;
        if context.namespace || context.aggregate || context.explicit_linkage {
            continue;
        }
        // Normalize only the selection/deduplication key. Emitted source must
        // preserve newlines that terminate C++ line comments.
        let key = candidate
            .signature
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if key.is_empty()
            || key.contains("::")
            || key.starts_with('#')
            || matches!(
                key.as_str(),
                "void setup()" | "void setup(void)" | "void loop()" | "void loop(void)"
            )
            || !seen.insert(key)
        {
            continue;
        }
        prototypes.push(candidate.signature);
    }
    Ok(prototypes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_primary_then_other_top_level_tabs_and_maps_each_tab() {
        let temp = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        let sketch = temp.path().join("Blink");
        fs::create_dir_all(sketch.join("nested")).unwrap();
        fs::write(
            sketch.join("Blink.ino"),
            "void setup() { helper(); }\nvoid loop() {}\n",
        )
        .unwrap();
        fs::write(sketch.join("zeta.ino"), "void zeta() {}\n").unwrap();
        fs::write(sketch.join("Alpha.ino"), "void helper(int value = 1) {}\n").unwrap();
        fs::write(
            sketch.join("nested").join("ignored.ino"),
            "void ignored() {}\n",
        )
        .unwrap();
        fs::write(sketch.join("old.pde"), "void old() {}\n").unwrap();

        let rendered = preprocess_disk(&sketch).unwrap();
        let names = rendered
            .documents
            .iter()
            .map(|document| {
                Path::new(&document.path)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string()
            })
            .collect::<Vec<_>>();
        assert_eq!(names, ["Blink.ino", "Alpha.ino", "zeta.ino"]);
        assert!(rendered.prototype_header.contains("void helper(int value)"));
        assert!(!rendered.translation_unit.contains("ignored.ino"));
        assert!(!rendered.translation_unit.contains("old.pde"));
        for document in &rendered.documents {
            assert!(rendered
                .translation_unit
                .contains(&format!("#line 1 \"{}\"", document.path)));
        }
    }

    #[test]
    fn live_snapshot_uses_unsaved_text_and_never_overwrites_sketch() {
        let temp = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        let sketch = temp.path().join("Sketch");
        fs::create_dir_all(&sketch).unwrap();
        let source = sketch.join("Sketch.ino");
        fs::write(
            &source,
            "void setup() { old_name(); }\nvoid loop() {}\nvoid old_name() {}\n",
        )
        .unwrap();
        let request = SnapshotRequest {
            sketch_dir: sketch.to_string_lossy().into_owned(),
            generation: 3,
            documents: vec![SnapshotDocument {
                path: source.to_string_lossy().into_owned(),
                version: 9,
                text: "void setup() { new_name(); }\nvoid loop() {}\nvoid new_name() {}\n"
                    .to_string(),
            }],
        };
        let manifest = write_live_snapshot(&request).unwrap();
        let cache = intellisense_dir(&sketch);
        assert_eq!(manifest.generation, 3);
        assert!(fs::read_to_string(cache.join("sketch.cpp"))
            .unwrap()
            .contains("new_name"));
        assert!(fs::read_to_string(&source).unwrap().contains("old_name"));
        assert_eq!(
            fs::read_to_string(sketch.join(".fastled/.gitignore")).unwrap(),
            "*\n!.gitignore\n"
        );
    }

    #[test]
    fn invalid_new_buffer_preserves_last_known_good_snapshot() {
        let temp = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        let sketch = temp.path().join("Sketch");
        fs::create_dir_all(&sketch).unwrap();
        let source = sketch.join("Sketch.ino");
        fs::write(
            &source,
            "void setup() {}\nvoid loop() {}\nvoid helper() {}\n",
        )
        .unwrap();
        write_live_snapshot(&SnapshotRequest {
            sketch_dir: sketch.to_string_lossy().into_owned(),
            generation: 1,
            documents: vec![],
        })
        .unwrap();
        let bad = SnapshotRequest {
            sketch_dir: sketch.to_string_lossy().into_owned(),
            generation: 2,
            documents: vec![SnapshotDocument {
                path: source.to_string_lossy().into_owned(),
                version: 2,
                text: "void setup(\n".to_string(),
            }],
        };
        assert!(write_live_snapshot(&bad).is_err());
        assert_eq!(
            read_manifest(&intellisense_dir(&sketch))
                .unwrap()
                .generation,
            1
        );
    }

    #[test]
    fn older_generation_cannot_replace_newer_snapshot() {
        let temp = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        let sketch = temp.path().join("Sketch");
        fs::create_dir_all(&sketch).unwrap();
        let source = sketch.join("Sketch.ino");
        fs::write(&source, "void setup() {}\nvoid loop() {}\n").unwrap();
        let newest = SnapshotRequest {
            sketch_dir: sketch.to_string_lossy().into_owned(),
            generation: 4,
            documents: vec![SnapshotDocument {
                path: source.to_string_lossy().into_owned(),
                version: 4,
                text: "void setup() { newest(); }\nvoid loop() {}\nvoid newest() {}\n".to_string(),
            }],
        };
        write_live_snapshot(&newest).unwrap();
        let stale = SnapshotRequest {
            sketch_dir: sketch.to_string_lossy().into_owned(),
            generation: 3,
            documents: vec![SnapshotDocument {
                path: source.to_string_lossy().into_owned(),
                version: 3,
                text: "void setup() { stale(); }\nvoid loop() {}\nvoid stale() {}\n".to_string(),
            }],
        };
        let manifest = write_live_snapshot(&stale).unwrap();
        assert_eq!(manifest.generation, 4);
        assert!(
            fs::read_to_string(intellisense_dir(&sketch).join("sketch.cpp"))
                .unwrap()
                .contains("newest")
        );
    }

    #[test]
    fn disk_topology_refresh_replaces_a_previous_live_generation() {
        let temp = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        let sketch = temp.path().join("Sketch");
        fs::create_dir_all(&sketch).unwrap();
        let source = sketch.join("Sketch.ino");
        fs::write(
            &source,
            "void setup() { disk(); }\nvoid loop() {}\nvoid disk() {}\n",
        )
        .unwrap();
        write_live_snapshot(&SnapshotRequest {
            sketch_dir: sketch.to_string_lossy().into_owned(),
            generation: 9,
            documents: vec![SnapshotDocument {
                path: source.to_string_lossy().into_owned(),
                version: 9,
                text: "void setup() { live(); }\nvoid loop() {}\nvoid live() {}\n".to_string(),
            }],
        })
        .unwrap();

        let (manifest, _) = write_disk_snapshot(&sketch).unwrap();
        assert_eq!(manifest.generation, 0);
        assert!(
            fs::read_to_string(intellisense_dir(&sketch).join("sketch.cpp"))
                .unwrap()
                .contains("disk")
        );
    }

    #[test]
    fn does_not_generate_cxx_prototypes_for_extern_c_definitions() {
        let source = r#"
            extern "C" {
                __attribute__((noinline, used)) void browser_hook() {}
            }
            void helper() {}
        "#;
        let prototypes = extract_function_prototypes(source).unwrap();
        assert!(prototypes
            .iter()
            .any(|prototype| prototype.contains("helper")));
        assert!(!prototypes
            .iter()
            .any(|prototype| prototype.contains("browser_hook")));
    }

    #[test]
    fn prototype_output_retains_comment_terminating_newlines() {
        let prototypes = extract_function_prototypes(
            "int helper(int x // parameter\n = 1) // header\n { return x; }",
        )
        .unwrap();
        assert_eq!(
            render_prototype_declarations(&prototypes),
            "int helper(int x // parameter\n) // header\n;\n"
        );
    }

    #[test]
    fn arduino_selection_keeps_global_helpers_in_order_without_duplicates() {
        let source = "void setup() {}\nvoid loop(void) {}\nnamespace n { void hidden() {} }\nstruct S { void member() {} };\nvoid helper() {}\nvoid helper() {}\nvoid second() {}";
        assert_eq!(
            extract_function_prototypes(source).unwrap(),
            ["void helper()", "void second()"]
        );
    }
}
