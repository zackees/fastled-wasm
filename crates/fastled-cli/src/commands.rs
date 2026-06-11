use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, RwLock};

use crate::build;
use crate::cli::{requested_init_ref, validate_init_ref_flags, Cli};
use crate::compile_stream::{
    ensure_compile_prerequisites, purge_fastled_cache, report_build_outcome,
    run_native_compile_streaming, selected_build_mode, send_sse, write_build_status,
};
use crate::debug_symbols;
use crate::dwarf_smoke::run_dwarf_source_smoke;
use crate::install;
use crate::keyboard;
use crate::project;
use crate::selection::prompt_for_example;
use crate::server;
use crate::viewer;
use crate::watcher;

// ---------------------------------------------------------------------------
// Compile + serve + watch (replaces Flask-based flow)
// ---------------------------------------------------------------------------

/// Compile a sketch, serve the output via the built-in HTTP server, and
/// watch for file changes to trigger recompilation.
///
/// Build output is streamed to the Tauri viewer in real time via SSE.
pub(crate) fn compile_and_serve(dir: &str, cli: &Cli) -> ExitCode {
    let sketch_dir = PathBuf::from(dir);
    if !sketch_dir.is_dir() {
        eprintln!("fastled: sketch directory does not exist: {dir}");
        return ExitCode::FAILURE;
    }

    // Ensure the emscripten + esbuild toolchains are installed before invoking
    // the native Rust build backend. The backend consumes the Rust-installed
    // directories via these environment variables.
    if let Err(message) = ensure_compile_prerequisites() {
        eprintln!("fastled: {message}");
        return ExitCode::FAILURE;
    }

    let output_dir = sketch_dir.join("fastled_js");
    if let Err(e) = std::fs::create_dir_all(&output_dir) {
        eprintln!(
            "fastled: could not create output directory {}: {e}",
            output_dir.display()
        );
        return ExitCode::FAILURE;
    }

    // Broadcast channel for SSE streaming to browser.
    let (tx, _rx) = tokio::sync::broadcast::channel::<String>(256);

    // Shared DWARF source resolver populated after the first successful build.
    let debug_symbols: server::DebugSymbolHandle = Arc::new(RwLock::new(None));

    // Write initial compiling status for polling fallback.
    write_build_status(&output_dir, "compiling", "Compiling...");

    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async {
        // Start the Rust HTTP server (background tokio task).
        let addr = match server::start_server(
            output_dir.clone(),
            0,
            Some(tx.clone()),
            debug_symbols.clone(),
        )
        .await
        {
            Ok(a) => a,
            Err(e) => {
                eprintln!("fastled: failed to start server: {e}");
                return ExitCode::FAILURE;
            }
        };

        let url = format!("http://{addr}");
        println!("Serving at {url}");

        let _viewer = match viewer::launch_tauri_viewer(&url) {
            Ok(process) => process,
            Err(e) => {
                eprintln!("fastled: Tauri viewer failed: {e:#}");
                return ExitCode::FAILURE;
            }
        };

        // --- Initial compilation ------------------------------------------------
        send_sse(
            &tx,
            r#"{"type":"status","status":"compiling","message":"Compiling..."}"#,
        );

        let success =
            run_native_compile_streaming(cli, &sketch_dir, cli.purge, &tx, &debug_symbols);
        report_build_outcome(&output_dir, &tx, success);

        // --- Watch mode ---------------------------------------------------------
        println!("\nWill recompile on sketch changes or space bar press.");
        println!("Press Ctrl+C to stop...");

        let watcher_result =
            watcher::FileWatcher::new(sketch_dir.clone(), watcher::DEFAULT_DEBOUNCE_MS);
        let mut file_watcher = match watcher_result {
            Ok(w) => w,
            Err(e) => {
                eprintln!("fastled: file watcher failed: {e}");
                tokio::signal::ctrl_c().await.ok();
                return ExitCode::SUCCESS;
            }
        };
        let rx = file_watcher.start();

        loop {
            let should_rebuild = match rx.recv_timeout(std::time::Duration::from_secs(1)) {
                Ok(changed) => {
                    println!("\nChanges detected in {changed:?}");
                    true
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => keyboard::check_for_space(),
                Err(_) => break,
            };

            if should_rebuild {
                println!("Compiling...");
                write_build_status(&output_dir, "compiling", "Recompiling...");
                send_sse(
                    &tx,
                    r#"{"type":"status","status":"compiling","message":"Recompiling..."}"#,
                );

                let success =
                    run_native_compile_streaming(cli, &sketch_dir, false, &tx, &debug_symbols);
                if report_build_outcome(&output_dir, &tx, success) {
                    println!("Recompilation successful!");
                } else {
                    eprintln!("Recompilation failed.");
                }
            }
        }

        file_watcher.stop();
        ExitCode::SUCCESS
    })
}

pub(crate) fn run_native_init(cli: &Cli, example: Option<&str>) -> ExitCode {
    if let Err(message) = validate_init_ref_flags(cli) {
        eprintln!("fastled: {message}");
        return ExitCode::FAILURE;
    }

    let cwd = match std::env::current_dir() {
        Ok(path) => path,
        Err(e) => {
            eprintln!("fastled: could not determine current directory: {e}");
            return ExitCode::FAILURE;
        }
    };
    let output_dir = cli
        .directory
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("fastled"));

    if let Some(local_repo) = project::find_fastled_repo_upwards(&cwd, 10) {
        println!("Using local FastLED repo at {}", local_repo.display());
        let selected_example = match example {
            Some(example) => example.to_string(),
            None => {
                if cli.no_interactive {
                    eprintln!("fastled: --init without an example requires interactive input");
                    return ExitCode::FAILURE;
                }
                match prompt_for_example(&local_repo) {
                    Ok(selected) => selected,
                    Err(err) => {
                        eprintln!("fastled: failed to select example: {err}");
                        return ExitCode::FAILURE;
                    }
                }
            }
        };
        // Pin the local repo so subsequent builds use the same checkout.
        let local_ref = local_repo.to_string_lossy().into_owned();
        return match project::init_example_from_repo(
            &local_repo,
            &selected_example,
            &output_dir,
            Some(local_ref.as_str()),
        ) {
            Ok(out) => {
                println!("Project initialized at {}", out.display());
                println!("\nInitialized FastLED project in {}", out.display());
                println!("Use 'fastled {}' to compile the project.", out.display());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("fastled: failed to initialize project: {e:#}");
                ExitCode::FAILURE
            }
        };
    }

    let mut ref_name = requested_init_ref(cli).map(str::to_owned);
    if ref_name.is_none() {
        ref_name = project::read_fastled_json_ref(&cwd)
            .or_else(|| project::read_fastled_json_ref(&output_dir));
        if let Some(saved_ref) = &ref_name {
            println!("Using saved ref '{saved_ref}' from fastled.json");
        }
    }

    let ref_display = ref_name.as_deref().unwrap_or("latest release");

    let repo_root = match install::ensure_fastled_repo(ref_name.as_deref()) {
        Ok(path) => path,
        Err(e) => {
            eprintln!("fastled: failed to fetch FastLED repo: {e:#}");
            return ExitCode::FAILURE;
        }
    };
    let resolved_ref = project::cached_repo_ref_name(&repo_root);
    let selected_example = match example {
        Some(example) => example.to_string(),
        None => {
            if cli.no_interactive {
                eprintln!("fastled: --init without an example requires interactive input");
                return ExitCode::FAILURE;
            }
            match prompt_for_example(&repo_root) {
                Ok(selected) => selected,
                Err(err) => {
                    eprintln!("fastled: failed to select example: {err}");
                    return ExitCode::FAILURE;
                }
            }
        }
    };

    println!(
        "Initializing project with example '{}' from FastLED repo ({ref_display})",
        selected_example
    );

    // Always pin the resolved ref into `fastled.json` so that a later
    // `fastled --just-compile <sketch>` builds against the same FastLED
    // checkout the sketch was scaffolded from. Without this, the build
    // path defaulted to `master`, which broke compiles when the latest
    // release used different header layouts than master.
    match project::init_example_from_repo(
        &repo_root,
        &selected_example,
        &output_dir,
        Some(resolved_ref.as_str()),
    ) {
        Ok(out) => {
            println!(
                "Saved ref '{resolved_ref}' to {}",
                out.join("fastled.json").display()
            );
            println!("Project initialized at {}", out.display());
            println!("\nInitialized FastLED project in {}", out.display());
            println!("Use 'fastled {}' to compile the project.", out.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("fastled: failed to initialize project: {e:#}");
            ExitCode::FAILURE
        }
    }
}

pub(crate) fn run_native_just_compile(cli: &Cli, dir: &str) -> ExitCode {
    if let Err(message) = ensure_compile_prerequisites() {
        eprintln!("fastled: {message}");
        return ExitCode::FAILURE;
    }

    if cli.purge {
        purge_fastled_cache(cli.fastled_path.as_deref());
    }

    let request = build::BuildRequest {
        sketch_dir: PathBuf::from(dir),
        build_mode: selected_build_mode(cli),
        profile: cli.profile,
        fastled_path: cli.fastled_path.as_ref().map(PathBuf::from),
        force_clean: cli.purge,
    };

    let result = match build::run_build(&request) {
        Ok(result) => result,
        Err(err) => {
            eprintln!("fastled: native compile path failed: {err:#}");
            return ExitCode::FAILURE;
        }
    };

    if !result.success {
        if !result.output.trim().is_empty() {
            eprintln!("{}", result.output.trim_end());
        }
        eprintln!("\nCompilation failed.");
        return ExitCode::FAILURE;
    }

    println!("\nCompilation successful!");
    println!("  Time: {:.2} seconds", result.sketch_time_secs);
    println!("  Wall time: {:.2} seconds", result.duration_secs);
    println!("  Strategy: {}", result.strategy);
    println!("  Output: {}", result.output_dir.display());
    ExitCode::SUCCESS
}

/// Serve a directory using the built-in Rust HTTP server.
///
/// This replaces the Flask-based `--serve-dir` implementation with a native
/// Rust server, eliminating the Python/Flask dependency for this code path.
pub(crate) fn serve_directory(dir: &str, launch_viewer: bool) -> ExitCode {
    let path = PathBuf::from(dir);
    if !path.is_dir() {
        eprintln!("fastled: --serve-dir path does not exist: {dir}");
        return ExitCode::FAILURE;
    }

    let resolver = match debug_symbols::read_debug_symbol_manifest(&path) {
        Ok(Some(config)) => Some(debug_symbols::DebugSymbolResolver::new(config)),
        Ok(None) => None,
        Err(err) => {
            eprintln!(
                "fastled: could not load {} from {}: {err:#}",
                debug_symbols::DWARF_ROOTS_MANIFEST,
                path.display()
            );
            None
        }
    };

    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async {
        let debug_symbols: server::DebugSymbolHandle = Arc::new(RwLock::new(resolver));
        let addr = match server::start_server(path.clone(), 0, None, debug_symbols).await {
            Ok(a) => a,
            Err(e) => {
                eprintln!("fastled: failed to start server: {e}");
                return ExitCode::FAILURE;
            }
        };

        let url = format!("http://{addr}");
        println!("Serving {dir} at {url}");
        println!("Press Ctrl+C to stop...");

        let _viewer = if launch_viewer {
            match viewer::launch_tauri_viewer(&url) {
                Ok(process) => Some(process),
                Err(e) => {
                    eprintln!("fastled: Tauri viewer failed: {e:#}");
                    return ExitCode::FAILURE;
                }
            }
        } else {
            None
        };

        // Wait for Ctrl+C.
        tokio::signal::ctrl_c().await.ok();
        println!("\nShutting down...");
        ExitCode::SUCCESS
    })
}

pub(crate) fn run_internal_dwarf_smoke(cli: &Cli) -> ExitCode {
    let Some(dir) = cli.directory.as_deref() else {
        eprintln!("fastled: --internal-dwarf-smoke requires a sketch directory");
        return ExitCode::FAILURE;
    };
    if selected_build_mode(cli) != build::BuildMode::Debug {
        eprintln!("fastled: --internal-dwarf-smoke must be run with --debug");
        return ExitCode::FAILURE;
    }
    if let Err(message) = ensure_compile_prerequisites() {
        eprintln!("fastled: {message}");
        return ExitCode::FAILURE;
    }
    if cli.purge {
        purge_fastled_cache(cli.fastled_path.as_deref());
    }

    let request = build::BuildRequest {
        sketch_dir: PathBuf::from(dir),
        build_mode: build::BuildMode::Debug,
        profile: cli.profile,
        fastled_path: cli.fastled_path.as_ref().map(PathBuf::from),
        force_clean: cli.purge,
    };
    let result = match build::run_build(&request) {
        Ok(result) => result,
        Err(err) => {
            eprintln!("fastled: native compile path failed: {err:#}");
            return ExitCode::FAILURE;
        }
    };
    if !result.success {
        eprintln!("{}", result.output.trim_end());
        eprintln!("\nCompilation failed.");
        return ExitCode::FAILURE;
    }

    match run_dwarf_source_smoke(&result.output_dir) {
        Ok(count) => {
            println!(
                "DWARF source smoke passed: resolved {count} embedded source paths via /dwarfsource"
            );
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("fastled: DWARF source smoke failed: {err:#}");
            ExitCode::FAILURE
        }
    }
}
