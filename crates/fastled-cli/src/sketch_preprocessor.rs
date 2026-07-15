//! Arduino `.ino` preprocessing shared by the WASM build and editor support.
//!
//! This is deliberately adapted from FastLED/fbuild's source scanner at
//! `1e75ccf5a4ca922b4d922a6da286b965fac8832d`. Keep generic parser fixes in
//! fbuild: this local adapter is transitional until fbuild publishes a stable
//! preprocessor crate/API that fastled-wasm can depend on directly (see #206).

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::{self, Read};
use std::path::Path;

use anyhow::{bail, Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tree_sitter::{Node, Parser};

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
    lock.lock_exclusive()
        .context("lock IntelliSense snapshot")?;

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

/// Tree-sitter based prototype collection copied/adapted from fbuild's
/// `source_scanner.rs`; regex-only prototype generation is intentionally not
/// used because Arduino sketches routinely use attributes/default arguments.
fn extract_function_prototypes(source: &str) -> Result<Vec<String>> {
    let mut parser = Parser::new();
    let language = tree_sitter_cpp::LANGUAGE.into();
    parser.set_language(&language).context("load C++ parser")?;
    let tree = parser.parse(source, None).context("parse Arduino sketch")?;
    if tree.root_node().has_error() {
        bail!("sketch has incomplete C++ syntax; retaining the last good IntelliSense prelude");
    }
    let mut candidates = Vec::new();
    collect_function_prototypes(tree.root_node(), source, &mut candidates);
    let mut seen = HashSet::new();
    Ok(candidates
        .into_iter()
        .filter(|prototype| seen.insert(prototype.clone()))
        .collect())
}

fn collect_function_prototypes(node: Node<'_>, source: &str, output: &mut Vec<String>) {
    if node.kind() == "function_definition" {
        if let Some(prototype) = prototype_from_definition(node, source) {
            output.push(prototype);
        }
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_function_prototypes(child, source, output);
    }
}

fn prototype_from_definition(node: Node<'_>, source: &str) -> Option<String> {
    if has_skipped_context(node) {
        return None;
    }
    let signature_node = node
        .parent()
        .filter(|parent| parent.kind() == "template_declaration")
        .unwrap_or(node);
    let body = node.child_by_field_name("body")?;
    let start = signature_node.start_byte();
    let signature = source.get(start..body.start_byte())?;
    let parameters = find_descendant(node, "parameter_list")?;
    let parameters_start = parameters.start_byte().checked_sub(start)?;
    let parameters_end = parameters.end_byte().checked_sub(start)?;
    let signature = normalize_signature(&strip_default_arguments(
        signature,
        parameters_start,
        parameters_end,
    ))?;
    if signature.contains("::")
        || signature.starts_with('#')
        || matches!(
            signature.trim(),
            "void setup()" | "void setup(void)" | "void loop()" | "void loop(void)"
        )
    {
        return None;
    }
    Some(signature)
}

fn has_skipped_context(node: Node<'_>) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "namespace_definition"
            | "class_specifier"
            | "struct_specifier"
            | "union_specifier"
            | "field_declaration_list" => return true,
            _ => current = parent.parent(),
        }
    }
    false
}

fn find_descendant<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == kind {
            return Some(child);
        }
        if let Some(found) = find_descendant(child, kind) {
            return Some(found);
        }
    }
    None
}

fn normalize_signature(signature: &str) -> Option<String> {
    let lines = signature
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    (!lines.is_empty()).then(|| lines.join(" "))
}

fn strip_default_arguments(signature: &str, start: usize, end: usize) -> String {
    let Some(parameters) = signature.get(start..end) else {
        return signature.to_string();
    };
    let Some(inner) = parameters
        .strip_prefix('(')
        .and_then(|value| value.strip_suffix(')'))
    else {
        return signature.to_string();
    };
    format!(
        "{}({}){}",
        &signature[..start],
        strip_defaults(inner),
        &signature[end..]
    )
}

fn strip_defaults(parameters: &str) -> String {
    let mut output = String::new();
    let mut skip_default = false;
    let mut depths = [0usize; 4]; // paren, bracket, brace, angle
    let mut quote = None;
    let mut escaped = false;
    for character in parameters.chars() {
        if let Some(delimiter) = quote {
            if !skip_default {
                output.push(character);
            }
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == delimiter {
                quote = None;
            }
            continue;
        }
        match character {
            '\'' | '"' => {
                if !skip_default {
                    output.push(character);
                }
                quote = Some(character);
            }
            '(' => {
                depths[0] += 1;
                if !skip_default {
                    output.push(character);
                }
            }
            ')' => {
                depths[0] = depths[0].saturating_sub(1);
                if !skip_default {
                    output.push(character);
                }
            }
            '[' => {
                depths[1] += 1;
                if !skip_default {
                    output.push(character);
                }
            }
            ']' => {
                depths[1] = depths[1].saturating_sub(1);
                if !skip_default {
                    output.push(character);
                }
            }
            '{' => {
                depths[2] += 1;
                if !skip_default {
                    output.push(character);
                }
            }
            '}' => {
                depths[2] = depths[2].saturating_sub(1);
                if !skip_default {
                    output.push(character);
                }
            }
            '<' => {
                depths[3] += 1;
                if !skip_default {
                    output.push(character);
                }
            }
            '>' => {
                depths[3] = depths[3].saturating_sub(1);
                if !skip_default {
                    output.push(character);
                }
            }
            '=' if depths.iter().all(|depth| *depth == 0) => {
                skip_default = true;
                output = output.trim_end().to_string();
            }
            ',' if depths.iter().all(|depth| *depth == 0) => {
                skip_default = false;
                output = output.trim_end().to_string();
                output.push(',');
            }
            _ if !skip_default => output.push(character),
            _ => {}
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_primary_then_other_top_level_tabs_and_maps_each_tab() {
        let temp = tempfile::tempdir().unwrap();
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
        let temp = tempfile::tempdir().unwrap();
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
        let temp = tempfile::tempdir().unwrap();
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
        let temp = tempfile::tempdir().unwrap();
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
        let temp = tempfile::tempdir().unwrap();
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
}
