use std::path::{Path, PathBuf};

use crate::build;
use crate::cli::Cli;
use crate::debug_symbols;
use crate::install;
use crate::server;

pub(crate) fn ensure_compile_prerequisites(include_app: bool) -> Result<(), String> {
    match install::ensure_emscripten_installed() {
        Ok(install_dir) => {
            std::env::set_var("FASTLED_EMSCRIPTEN_DIR", &install_dir);
            // Compatibility exports for any remaining callers that still read
            // the historical clang-tool-chain environment variables. The Rust
            // build backend invokes the installed Emscripten scripts directly.
            if let Some(root) = install_dir
                .parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.parent())
                .and_then(|p| p.parent())
            {
                std::env::set_var("CLANG_TOOL_CHAIN_DOWNLOAD_PATH", root);
            }
        }
        Err(e) => {
            return Err(format!("emscripten toolchain install failed: {e:#}"));
        }
    }
    if include_app {
        match install::ensure_esbuild_installed() {
            Ok(esbuild_path) => {
                std::env::set_var("FASTLED_ESBUILD_PATH", &esbuild_path);
            }
            Err(e) => {
                return Err(format!("esbuild install failed: {e:#}"));
            }
        }
    }
    Ok(())
}

pub(crate) fn selected_build_mode(cli: &Cli) -> build::BuildMode {
    if cli.debug {
        build::BuildMode::Debug
    } else if cli.release {
        build::BuildMode::Release
    } else {
        build::BuildMode::Quick
    }
}

pub(crate) fn purge_fastled_cache(fastled_path: Option<&str>) {
    if let Some(home) = dirs::home_dir() {
        let cache_dir = home.join(".fastled").join("cache");
        if cache_dir.exists() {
            match std::fs::remove_dir_all(&cache_dir) {
                Ok(()) => println!("Purged FastLED cache: {}", cache_dir.display()),
                Err(err) => eprintln!(
                    "fastled: failed to purge cache {}: {err}",
                    cache_dir.display()
                ),
            }
        } else {
            println!("No FastLED cache to purge.");
        }
    }

    if let Some(path) = fastled_path {
        let fastled_build = Path::new(path).join(".build");
        if let Ok(entries) = std::fs::read_dir(&fastled_build) {
            for entry in entries.flatten() {
                let wasm_dir = entry.path();
                let Some(name) = wasm_dir.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                if !name.starts_with("meson-wasm-") || !wasm_dir.is_dir() {
                    continue;
                }
                let runtime_cache = wasm_dir.join("dynamic-runtime-cache");
                if runtime_cache.exists() {
                    match std::fs::remove_dir_all(&runtime_cache) {
                        Ok(()) => println!("Purged: {}", runtime_cache.display()),
                        Err(err) => eprintln!(
                            "fastled: failed to purge {}: {err}",
                            runtime_cache.display()
                        ),
                    }
                }
                for stale in [
                    "wasm_ld_args.json",
                    "wasm_ld_args.key",
                    "fastled_glue.js",
                    "js_glue_fingerprint",
                    "link_environment_fingerprint",
                    "libemscripten_js_symbols.so",
                ] {
                    let stale_file = wasm_dir.join(stale);
                    if stale_file.exists() {
                        match std::fs::remove_file(&stale_file) {
                            Ok(()) => println!("Purged: {}", stale_file.display()),
                            Err(err) => eprintln!(
                                "fastled: failed to purge {}: {err}",
                                stale_file.display()
                            ),
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Build-status IPC (Rust writes, loading page polls)
// ---------------------------------------------------------------------------

pub(crate) fn write_build_status(output_dir: &Path, status: &str, message: &str) {
    let status_file = output_dir.join("build-status.json");
    let json = format!(
        r#"{{"status":"{}","message":"{}"}}"#,
        status,
        message.replace('\\', "\\\\").replace('"', "\\\"")
    );
    let _ = std::fs::write(status_file, json);
}

// ---------------------------------------------------------------------------
// Streaming compile (captures output line-by-line, sends via broadcast)
// ---------------------------------------------------------------------------

/// Escape a string for JSON embedding.
pub(crate) fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

/// Send an SSE event through the broadcast channel.
pub(crate) fn send_sse(tx: &tokio::sync::broadcast::Sender<String>, json: &str) {
    let _ = tx.send(json.to_string());
}

pub(crate) fn emit_build_log(
    tx: &tokio::sync::broadcast::Sender<String>,
    line: &str,
    stream: &str,
) {
    if stream == "stderr" {
        eprintln!("{line}");
    } else {
        println!("{line}");
    }
    send_sse(
        tx,
        &format!(
            r#"{{"type":"log","line":"{}","stream":"{}"}}"#,
            json_escape(line),
            stream
        ),
    );
}

pub(crate) fn emit_build_result_logs(
    result: &build::BuildResult,
    tx: &tokio::sync::broadcast::Sender<String>,
) {
    let stream = if result.success { "stdout" } else { "stderr" };
    for line in result.output.lines() {
        emit_build_log(tx, line, stream);
    }

    if result.success {
        emit_build_log(
            tx,
            &format!(
                "Build finished in {:.2}s (strategy: {}, output: {})",
                result.sketch_time_secs,
                result.strategy,
                result.output_dir.display()
            ),
            "stdout",
        );
    }
}

/// Publish the build outcome to polling (`build-status.json`) and SSE
/// clients. `success` is only reported when the build claims success *and*
/// `index.html` actually exists on disk — the loading page reloads on
/// `success`, and reloading without artifacts would wipe the error log (#153).
/// Returns the effective success that was reported.
pub(crate) fn report_build_outcome(
    output_dir: &Path,
    tx: &tokio::sync::broadcast::Sender<String>,
    build_ok: bool,
    app_required: bool,
) -> bool {
    let index_exists = output_dir.join("index.html").is_file();
    if build_ok && (!app_required || index_exists) {
        write_build_status(output_dir, "success", "Done");
        send_sse(
            tx,
            r#"{"type":"status","status":"success","message":"Done"}"#,
        );
        return true;
    }
    let message = if build_ok && app_required {
        "Build completed but no index.html was produced"
    } else {
        "Compilation failed"
    };
    write_build_status(output_dir, "error", message);
    send_sse(
        tx,
        &format!(
            r#"{{"type":"status","status":"error","message":"{}"}}"#,
            json_escape(message)
        ),
    );
    false
}

/// Run the native build path and mirror its output to the terminal + SSE.
pub(crate) fn run_native_compile_streaming(
    cli: &Cli,
    sketch_dir: &Path,
    force_clean: bool,
    tx: &tokio::sync::broadcast::Sender<String>,
    debug_symbols: &server::DebugSymbolHandle,
) -> bool {
    if force_clean {
        purge_fastled_cache(cli.fastled_path.as_deref());
    }

    let request = build::BuildRequest {
        sketch_dir: sketch_dir.to_path_buf(),
        build_mode: selected_build_mode(cli),
        profile: cli.profile,
        fastled_path: cli.fastled_path.as_ref().map(PathBuf::from),
        force_clean,
        emit_clangd: cli.clangd,
        no_app: cli.no_app,
        link_mode: cli.link_mode,
    };

    let log = |line: &str, stream: &str| emit_build_log(tx, line, stream);
    match build::run_build_streaming(&request, &log) {
        Ok(result) => {
            emit_build_result_logs(&result, tx);
            if result.success {
                update_debug_symbol_resolver(
                    debug_symbols,
                    sketch_dir,
                    result.fastled_dir.as_deref(),
                    result.emsdk_root.as_deref(),
                );
            }
            result.success
        }
        Err(err) => {
            emit_build_log(
                tx,
                &format!("fastled: native compile path failed: {err:#}"),
                "stderr",
            );
            false
        }
    }
}

pub(crate) fn update_debug_symbol_resolver(
    handle: &server::DebugSymbolHandle,
    sketch_dir: &Path,
    fastled_dir: Option<&Path>,
    emsdk_root: Option<&Path>,
) {
    let resolver =
        debug_symbols::DebugSymbolResolver::new(debug_symbols::load_debug_symbol_config(
            sketch_dir.to_path_buf(),
            fastled_dir.map(Path::to_path_buf),
            emsdk_root
                .map(Path::to_path_buf)
                .or_else(debug_symbols::guess_emsdk_path),
        ));
    if let Ok(mut guard) = handle.write() {
        *guard = Some(resolver);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_status(dir: &Path) -> String {
        std::fs::read_to_string(dir.join("build-status.json")).unwrap()
    }

    #[test]
    fn report_build_outcome_success_requires_index_html() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, mut rx) = tokio::sync::broadcast::channel::<String>(8);

        assert!(!report_build_outcome(dir.path(), &tx, true, true));
        assert!(read_status(dir.path()).contains("\"error\""));
        let event = rx.try_recv().unwrap();
        assert!(event.contains("\"status\":\"error\""), "got: {event}");

        std::fs::write(dir.path().join("index.html"), "<html></html>").unwrap();
        assert!(report_build_outcome(dir.path(), &tx, true, true));
        assert!(read_status(dir.path()).contains("\"success\""));
        let event = rx.try_recv().unwrap();
        assert!(event.contains("\"status\":\"success\""), "got: {event}");
    }

    #[test]
    fn report_build_outcome_failure_is_error_even_with_stale_index() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.html"), "<html></html>").unwrap();
        let (tx, mut rx) = tokio::sync::broadcast::channel::<String>(8);

        assert!(!report_build_outcome(dir.path(), &tx, false, true));
        assert!(read_status(dir.path()).contains("\"error\""));
        let event = rx.try_recv().unwrap();
        assert!(event.contains("Compilation failed"), "got: {event}");
    }

    #[test]
    fn report_build_outcome_allows_success_without_app() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, mut rx) = tokio::sync::broadcast::channel::<String>(8);

        assert!(report_build_outcome(dir.path(), &tx, true, false));
        assert!(read_status(dir.path()).contains("\"success\""));
        let event = rx.try_recv().unwrap();
        assert!(event.contains("\"status\":\"success\""), "got: {event}");
    }
}
