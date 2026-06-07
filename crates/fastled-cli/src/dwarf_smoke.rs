use std::collections::BTreeSet;
use std::path::Path;
use std::sync::{Arc, RwLock};

use anyhow::Context;

use crate::debug_symbols;
use crate::server;

pub(crate) fn run_dwarf_source_smoke(output_dir: &Path) -> anyhow::Result<usize> {
    let config = debug_symbols::read_debug_symbol_manifest(output_dir)?.ok_or_else(|| {
        anyhow::anyhow!(
            "missing {} in {}",
            debug_symbols::DWARF_ROOTS_MANIFEST,
            output_dir.display()
        )
    })?;
    let paths = collect_debug_source_paths(output_dir, &config)?;
    if paths.is_empty() {
        anyhow::bail!(
            "no debug source paths found in {} or {}",
            output_dir.join("fastled.wasm").display(),
            output_dir.join("fastled.wasm.map").display()
        );
    }

    let resolver = debug_symbols::DebugSymbolResolver::new(config);
    let debug_symbols: server::DebugSymbolHandle = Arc::new(RwLock::new(Some(resolver)));
    let output_dir = output_dir.to_path_buf();
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async move {
        let addr = server::start_server(output_dir, 0, None, debug_symbols).await?;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let client = reqwest::Client::new();
        for path in &paths {
            let resp = client
                .post(format!("http://{addr}/dwarfsource"))
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(serde_json::json!({ "path": path }).to_string())
                .send()
                .await
                .with_context(|| format!("POST /dwarfsource for {path}"))?;
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                anyhow::bail!("/dwarfsource rejected {path} with {status}: {body}");
            }
        }
        Ok(paths.len())
    })
}

pub(crate) fn collect_debug_source_paths(
    output_dir: &Path,
    config: &debug_symbols::DebugSymbolConfig,
) -> anyhow::Result<BTreeSet<String>> {
    let prefixes = debug_source_prefixes(config);
    let mut paths = BTreeSet::new();

    let wasm_path = output_dir.join("fastled.wasm");
    let wasm =
        std::fs::read(&wasm_path).with_context(|| format!("read {}", wasm_path.display()))?;
    for candidate in extract_ascii_strings(&wasm) {
        insert_debug_source_candidate(&mut paths, &prefixes, &candidate);
    }

    let source_map_path = output_dir.join("fastled.wasm.map");
    if source_map_path.is_file() {
        let json = std::fs::read_to_string(&source_map_path)
            .with_context(|| format!("read {}", source_map_path.display()))?;
        let parsed: serde_json::Value = serde_json::from_str(&json)
            .with_context(|| format!("parse {}", source_map_path.display()))?;
        if let Some(sources) = parsed.get("sources").and_then(serde_json::Value::as_array) {
            for source in sources.iter().filter_map(serde_json::Value::as_str) {
                insert_debug_source_candidate(&mut paths, &prefixes, source);
            }
        }
    }

    Ok(paths)
}

fn debug_source_prefixes(config: &debug_symbols::DebugSymbolConfig) -> Vec<String> {
    let mut prefixes = Vec::new();
    for prefix in [
        config.prefixes.sketch_prefix.as_str(),
        config.prefixes.fastled_prefix.as_str(),
        config.prefixes.dwarf_prefix.as_str(),
    ] {
        push_debug_source_prefix(&mut prefixes, prefix);
    }
    for (prefix, _) in config.source_roots() {
        push_debug_source_prefix(&mut prefixes, &prefix);
    }
    prefixes
}

fn push_debug_source_prefix(prefixes: &mut Vec<String>, prefix: &str) {
    let mut normalized = prefix.trim_matches('/').replace('\\', "/");
    if normalized.is_empty() {
        return;
    }
    normalized.push('/');
    if !prefixes.iter().any(|existing| existing == &normalized) {
        prefixes.push(normalized);
    }
}

fn insert_debug_source_candidate(
    paths: &mut BTreeSet<String>,
    prefixes: &[String],
    candidate: &str,
) {
    let normalized = candidate
        .trim()
        .replace('\\', "/")
        .trim_start_matches('/')
        .to_string();
    if normalized.is_empty() || normalized.split('/').any(|segment| segment == "..") {
        return;
    }
    if prefixes
        .iter()
        .any(|prefix| normalized.starts_with(prefix) || normalized.contains(&format!("/{prefix}")))
    {
        paths.insert(normalized);
    }
}

fn extract_ascii_strings(bytes: &[u8]) -> Vec<String> {
    let mut strings = Vec::new();
    let mut current = Vec::new();
    for &byte in bytes {
        if (0x20..=0x7e).contains(&byte) {
            current.push(byte);
        } else if current.len() >= 4 {
            strings.push(String::from_utf8_lossy(&current).into_owned());
            current.clear();
        } else {
            current.clear();
        }
    }
    if current.len() >= 4 {
        strings.push(String::from_utf8_lossy(&current).into_owned());
    }
    strings
}
