#![recursion_limit = "512"]

use clap::Parser;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

mod archive;
mod build;
pub mod frontend;
pub mod install;
mod keyboard;
pub mod project;
pub mod runtime;
mod server;
pub mod viewer;
pub mod wasm_build;
mod watcher;

const DEFAULT_EXAMPLE: &str = "wasm";

fn ensure_compile_prerequisites() -> Result<(), String> {
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
    match install::ensure_esbuild_installed() {
        Ok(esbuild_path) => {
            std::env::set_var("FASTLED_ESBUILD_PATH", &esbuild_path);
        }
        Err(e) => {
            return Err(format!("esbuild install failed: {e:#}"));
        }
    }
    Ok(())
}

fn selected_build_mode(cli: &Cli) -> build::BuildMode {
    if cli.debug {
        build::BuildMode::Debug
    } else if cli.release {
        build::BuildMode::Release
    } else {
        build::BuildMode::Quick
    }
}

fn purge_fastled_cache(fastled_path: Option<&str>) {
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

fn write_build_status(output_dir: &Path, status: &str, message: &str) {
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
fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

/// Send an SSE event through the broadcast channel.
fn send_sse(tx: &tokio::sync::broadcast::Sender<String>, json: &str) {
    let _ = tx.send(json.to_string());
}

fn emit_build_log(tx: &tokio::sync::broadcast::Sender<String>, line: &str, stream: &str) {
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

fn emit_build_result_logs(
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

/// Run the native build path and mirror its output to the terminal + SSE.
fn run_native_compile_streaming(
    cli: &Cli,
    sketch_dir: &Path,
    force_clean: bool,
    tx: &tokio::sync::broadcast::Sender<String>,
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
    };

    match build::run_build(&request) {
        Ok(result) => {
            emit_build_result_logs(&result, tx);
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

// ---------------------------------------------------------------------------
// Compile + serve + watch (replaces Flask-based flow)
// ---------------------------------------------------------------------------

/// Compile a sketch, serve the output via the built-in HTTP server, and
/// watch for file changes to trigger recompilation.
///
/// Build output is streamed to the Tauri viewer in real time via SSE.
fn compile_and_serve(dir: &str, cli: &Cli) -> ExitCode {
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

    // Write initial compiling status for polling fallback.
    write_build_status(&output_dir, "compiling", "Compiling...");

    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async {
        // Start the Rust HTTP server (background tokio task).
        let addr = match server::start_server(output_dir.clone(), 0, Some(tx.clone())).await {
            Ok(a) => a,
            Err(e) => {
                eprintln!("fastled: failed to start server: {e}");
                return ExitCode::FAILURE;
            }
        };

        let url = format!("http://{addr}");
        println!("Serving at {url}");

        let _viewer = match viewer::launch_tauri_viewer(&output_dir) {
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

        let success = run_native_compile_streaming(cli, &sketch_dir, cli.purge, &tx);

        if success {
            write_build_status(&output_dir, "success", "Done");
            send_sse(
                &tx,
                r#"{"type":"status","status":"success","message":"Done"}"#,
            );
        } else {
            write_build_status(&output_dir, "error", "Compilation failed");
            send_sse(
                &tx,
                r#"{"type":"status","status":"error","message":"Compilation failed"}"#,
            );
        }

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

                let success = run_native_compile_streaming(cli, &sketch_dir, false, &tx);
                if success {
                    println!("Recompilation successful!");
                    write_build_status(&output_dir, "success", "Done");
                    send_sse(
                        &tx,
                        r#"{"type":"status","status":"success","message":"Done"}"#,
                    );
                } else {
                    eprintln!("Recompilation failed.");
                    write_build_status(&output_dir, "error", "Compilation failed");
                    send_sse(
                        &tx,
                        r#"{"type":"status","status":"error","message":"Compilation failed"}"#,
                    );
                }
            }
        }

        file_watcher.stop();
        ExitCode::SUCCESS
    })
}

/// FastLED WASM compilation CLI.
///
/// Rust front-end for FastLED WASM workflows.
///
/// Native Rust owns the full user-facing CLI surface, including compile
/// orchestration through `build.rs`.
#[derive(Parser, Debug)]
#[command(
    name = "fastled",
    version,
    about = "FastLED WASM compilation CLI",
    long_about = None,
    // Stop Rust clap from eating flags it doesn't recognise; we want to be
    // a strict mirror, so every flag is declared explicitly.
)]
struct Cli {
    /// Directory containing the FastLED sketch to compile.
    directory: Option<String>,

    /// Serve an existing directory without compiling a sketch.
    #[arg(long, value_name = "DIR")]
    serve_dir: Option<String>,

    /// Initialize a FastLED sketch in the current directory.
    /// An optional example name may be provided (e.g. --init Blink).
    #[arg(long, value_name = "EXAMPLE", num_args = 0..=1, default_missing_value = "__init__")]
    init: Option<String>,

    /// Just compile; skip opening the browser and watching for changes.
    #[arg(long)]
    just_compile: bool,

    /// Enable profiling of the C++ build system used for WASM compilation.
    #[arg(long)]
    profile: bool,

    /// Install the FastLED development environment with VSCode configuration.
    #[arg(long)]
    install: bool,

    /// Run in dry-run mode (simulate actions without making changes).
    #[arg(long)]
    dry_run: bool,

    /// Run in non-interactive mode (fail instead of prompting for input).
    #[arg(long)]
    no_interactive: bool,

    /// Disable HTTPS and use HTTP for the local server.
    #[arg(long)]
    no_https: bool,

    /// Use the latest release when initialising examples with --init (default behaviour).
    #[arg(long)]
    latest: bool,

    /// Use a specific branch when initialising examples with --init.
    #[arg(long, value_name = "BRANCH")]
    branch: Option<String>,

    /// Use a specific commit SHA when initialising examples with --init.
    #[arg(long, value_name = "SHA")]
    commit: Option<String>,

    /// Path to the FastLED library for native compilation.
    #[arg(long, value_name = "PATH")]
    fastled_path: Option<String>,

    /// Purge the cached FastLED repo, forcing a fresh re-download on next build.
    #[arg(long)]
    purge: bool,

    /// Internal plumbing flag: ensure the FastLED repo for the given ref
    /// (defaults to latest release) is downloaded and extracted, print the
    /// local path to stdout, and exit. Used by the Python `Api.project_init`
    /// path so the Python side never has to do an HTTP download.
    #[arg(long, value_name = "REF", num_args = 0..=1, default_missing_value = "__latest__", hide = true)]
    internal_ensure_fastled_repo: Option<String>,

    // Build mode (mutually exclusive).
    /// Build in debug mode.
    #[arg(long, conflicts_with_all = ["quick", "release"])]
    debug: bool,

    /// Build in quick mode (default).
    #[arg(long, conflicts_with_all = ["debug", "release"])]
    quick: bool,

    /// Build in optimised release mode.
    #[arg(long, conflicts_with_all = ["debug", "quick"])]
    release: bool,
}

fn validate_init_ref_flags(cli: &Cli) -> Result<(), &'static str> {
    if cli.latest && (cli.branch.is_some() || cli.commit.is_some()) {
        return Err("--latest cannot be used with --branch or --commit");
    }
    Ok(())
}

fn requested_init_ref(cli: &Cli) -> Option<&str> {
    if cli.latest {
        None
    } else {
        cli.commit.as_deref().or(cli.branch.as_deref())
    }
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn canonical_display_path(path: &Path) -> String {
    path.canonicalize()
        .map(|resolved| {
            let text = display_path(&resolved);
            #[cfg(windows)]
            {
                text.strip_prefix(r"\\?\").unwrap_or(&text).to_string()
            }
            #[cfg(not(windows))]
            {
                text
            }
        })
        .unwrap_or_else(|_| display_path(path))
}

fn detect_local_fastled_path() -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    project::find_fastled_repo_upwards(&cwd, 10).map(|path| canonical_display_path(&path))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptChoice {
    Selected(String),
    Narrowed(Vec<String>),
    Retry,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SketchSelection {
    Selected(String),
    Prompt(Vec<String>),
    None,
}

fn option_basename(option: &str) -> &str {
    Path::new(option)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(option)
}

fn fuzzy_match_options(input: &str, options: &[String]) -> Vec<String> {
    let basenames: Vec<String> = options
        .iter()
        .map(|option| option_basename(option).to_string())
        .collect();
    let basename_refs: Vec<&str> = basenames.iter().map(String::as_str).collect();
    let fuzzy_matches = project::best_sketch_match(input, &basename_refs);
    if fuzzy_matches.is_empty() {
        return Vec::new();
    }

    let mut results = Vec::new();
    for (index, basename) in basenames.iter().enumerate() {
        if fuzzy_matches.iter().any(|candidate| candidate == basename) {
            results.push(options[index].clone());
        }
    }
    results
}

pub fn resolve_prompt_choice(
    input: &str,
    options: &[String],
    default_index: usize,
) -> PromptChoice {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return PromptChoice::Selected(options[default_index].clone());
    }

    if let Ok(index) = trimmed.parse::<usize>() {
        if (1..=options.len()).contains(&index) {
            return PromptChoice::Selected(options[index - 1].clone());
        }
    }

    if let Some(exact) = options
        .iter()
        .find(|option| option.eq_ignore_ascii_case(trimmed))
    {
        return PromptChoice::Selected(exact.clone());
    }

    let input_lower = trimmed.to_lowercase();
    let partial_matches: Vec<String> = options
        .iter()
        .filter(|option| option.to_lowercase().contains(&input_lower))
        .cloned()
        .collect();
    match partial_matches.len() {
        1 => return PromptChoice::Selected(partial_matches[0].clone()),
        n if n > 1 => return PromptChoice::Narrowed(partial_matches),
        _ => {}
    }

    let fuzzy_matches = fuzzy_match_options(trimmed, options);
    match fuzzy_matches.len() {
        1 => PromptChoice::Selected(fuzzy_matches[0].clone()),
        n if n > 1 => PromptChoice::Narrowed(fuzzy_matches),
        _ => PromptChoice::Retry,
    }
}

pub fn prepare_sketch_selection(
    mut sketch_directories: Vec<PathBuf>,
    cwd_is_fastled: bool,
    is_followup: bool,
) -> SketchSelection {
    if cwd_is_fastled {
        sketch_directories.retain(|path| {
            let text = path.to_string_lossy().replace('\\', "/");
            !matches!(text.as_str(), "src" | "dev" | "tests")
        });
    }

    match sketch_directories.len() {
        0 => SketchSelection::None,
        1 => SketchSelection::Selected(display_path(&sketch_directories[0])),
        _ if !is_followup && sketch_directories.len() > 4 => SketchSelection::None,
        _ => SketchSelection::Prompt(
            sketch_directories
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
        ),
    }
}

fn prompt_for_choice(
    options: &[String],
    prompt: &str,
    default_index: usize,
) -> Result<String, String> {
    if options.is_empty() {
        return Err("no options available".to_string());
    }
    if options.len() == 1 {
        return Ok(options[0].clone());
    }

    let mut current_options = options.to_vec();
    let mut current_prompt = prompt.to_string();
    let mut current_default = default_index.min(current_options.len() - 1);

    loop {
        println!("\n{current_prompt}");
        for (index, option) in current_options.iter().enumerate() {
            if index == current_default {
                println!("  [{}]: [{}]", index + 1, option);
            } else {
                println!("  [{}]: {}", index + 1, option);
            }
        }

        let default_option = &current_options[current_default];
        print!("\nEnter number or name (default: [{default_option}]): ");
        std::io::stdout()
            .flush()
            .map_err(|err| format!("failed to flush prompt: {err}"))?;

        let mut input = String::new();
        std::io::stdin()
            .read_line(&mut input)
            .map_err(|err| format!("failed to read selection: {err}"))?;

        match resolve_prompt_choice(&input, &current_options, current_default) {
            PromptChoice::Selected(choice) => return Ok(choice),
            PromptChoice::Narrowed(matches) => {
                let query = input.trim();
                let is_partial = current_options
                    .iter()
                    .filter(|option| option.to_lowercase().contains(&query.to_lowercase()))
                    .count()
                    > 1;
                current_prompt = if is_partial {
                    format!("Multiple partial matches for '{query}':")
                } else {
                    format!("Multiple fuzzy matches for '{query}':")
                };
                current_options = matches;
                current_default = 0;
            }
            PromptChoice::Retry => {
                println!("No match found for '{}'. Please try again.", input.trim());
            }
        }
    }
}

fn prompt_for_example(repo_root: &Path) -> Result<String, String> {
    let examples = project::collect_examples(&repo_root.join("examples"));
    if examples.is_empty() {
        return Err(format!(
            "no examples found in FastLED repo {}",
            repo_root.display()
        ));
    }
    let default_index = examples
        .iter()
        .position(|example| example.eq_ignore_ascii_case(DEFAULT_EXAMPLE))
        .unwrap_or(0);
    prompt_for_choice(&examples, "Available examples:", default_index)
}

fn select_sketch_directory(
    mut sketch_directories: Vec<PathBuf>,
    cwd_is_fastled: bool,
    no_interactive: bool,
) -> Result<Option<String>, String> {
    if cwd_is_fastled {
        sketch_directories.retain(|path| {
            let text = path.to_string_lossy().replace('\\', "/");
            !matches!(text.as_str(), "src" | "dev" | "tests")
        });
    }

    match sketch_directories.len() {
        0 => Ok(None),
        1 => Ok(Some(display_path(&sketch_directories[0]))),
        _ if no_interactive => Err(
            "multiple sketch directories found; specify one explicitly when using --no-interactive"
                .to_string(),
        ),
        _ => {
            let options: Vec<String> = sketch_directories
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect();
            prompt_for_choice(&options, "Multiple Directories found, choose one:", 0).map(Some)
        }
    }
}

fn resolve_compile_directory(cli: &Cli) -> Result<Option<String>, String> {
    let cwd = std::env::current_dir()
        .map_err(|err| format!("could not determine current directory: {err}"))?;

    let Some(directory) = &cli.directory else {
        if project::looks_like_sketch_directory(&cwd, false) {
            return Ok(Some(display_path(&cwd)));
        }
        let cwd_is_fastled = project::is_fastled_repo(&cwd);
        return select_sketch_directory(
            project::find_sketches(&cwd),
            cwd_is_fastled,
            cli.no_interactive,
        );
    };

    let provided_path = PathBuf::from(directory);
    if provided_path.is_file() {
        let parent = provided_path.parent().map(PathBuf::from);
        if let Some(parent) = parent {
            if project::looks_like_sketch_directory(&parent, false) {
                return Ok(Some(canonical_display_path(&parent)));
            }
        }
        return Ok(Some(directory.clone()));
    }

    if provided_path.exists() {
        return Ok(Some(canonical_display_path(&provided_path)));
    }

    project::find_sketch_by_partial_name(directory, &cwd)
        .map(|matched| display_path(&matched))
        .map(Some)
        .map_err(|err| err.to_string())
}

fn run_native_init(cli: &Cli, example: Option<&str>) -> ExitCode {
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
        return match project::init_example_from_repo(
            &local_repo,
            &selected_example,
            &output_dir,
            None,
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

    match project::init_example_from_repo(
        &repo_root,
        &selected_example,
        &output_dir,
        ref_name.as_ref().map(|_| resolved_ref.as_str()),
    ) {
        Ok(out) => {
            if ref_name.is_some() {
                println!(
                    "Saved ref '{resolved_ref}' to {}",
                    out.join("fastled.json").display()
                );
            }
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

fn run_native_just_compile(cli: &Cli, dir: &str) -> ExitCode {
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
fn serve_directory(dir: &str) -> ExitCode {
    let path = PathBuf::from(dir);
    if !path.is_dir() {
        eprintln!("fastled: --serve-dir path does not exist: {dir}");
        return ExitCode::FAILURE;
    }

    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async {
        let addr = match server::start_server(path.clone(), 0, None).await {
            Ok(a) => a,
            Err(e) => {
                eprintln!("fastled: failed to start server: {e}");
                return ExitCode::FAILURE;
            }
        };

        let url = format!("http://{addr}");
        println!("Serving {dir} at {url}");
        println!("Press Ctrl+C to stop...");

        let _viewer = match viewer::launch_tauri_viewer(&path) {
            Ok(process) => process,
            Err(e) => {
                eprintln!("fastled: Tauri viewer failed: {e:#}");
                return ExitCode::FAILURE;
            }
        };

        // Wait for Ctrl+C.
        tokio::signal::ctrl_c().await.ok();
        println!("\nShutting down...");
        ExitCode::SUCCESS
    })
}

/// Library entry point invoked by the `fastled-rs` binary.
pub fn run() -> ExitCode {
    let mut cli = Cli::parse();

    if let Err(message) = validate_init_ref_flags(&cli) {
        eprintln!("fastled: {message}");
        return ExitCode::FAILURE;
    }

    // Hidden plumbing for the Python side: download the FastLED repo and
    // print the local path. No further work.
    if let Some(ref ref_str) = cli.internal_ensure_fastled_repo {
        let ref_opt = if ref_str == "__latest__" {
            None
        } else {
            Some(ref_str.as_str())
        };
        return match install::ensure_fastled_repo(ref_opt) {
            Ok(path) => {
                println!("{}", path.display());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("fastled: failed to fetch FastLED repo: {e:#}");
                ExitCode::FAILURE
            }
        };
    }

    // Handle --serve-dir natively with the Rust HTTP server (no Python needed).
    if let Some(ref serve_dir) = cli.serve_dir {
        return serve_directory(serve_dir);
    }

    if cli.install {
        match install::run_install(install::InstallOptions {
            dry_run: cli.dry_run,
            no_interactive: cli.no_interactive,
        }) {
            Ok(outcome) => {
                cli.install = false;
                if !outcome.launch_after {
                    return ExitCode::SUCCESS;
                }
            }
            Err(err) => {
                eprintln!("fastled: installation failed: {err:#}");
                return ExitCode::FAILURE;
            }
        }
    }

    if let Some(ref init_value) = cli.init {
        return run_native_init(
            &cli,
            if init_value == "__init__" {
                None
            } else {
                Some(init_value.as_str())
            },
        );
    }

    if cli.init.is_none() {
        if cli.fastled_path.is_none() {
            cli.fastled_path = detect_local_fastled_path();
        }

        match resolve_compile_directory(&cli) {
            Ok(Some(directory)) => cli.directory = Some(directory),
            Ok(None) => {}
            Err(message) => {
                eprintln!("fastled: {message}");
                return ExitCode::FAILURE;
            }
        }
    }

    // Normal compilation flow with Rust HTTP server + watch mode.
    // The Tauri viewer uses the same compile_and_serve() infrastructure so
    // compilation output is streamed via SSE in real time.
    //
    // Conditions:
    //  - a sketch directory was provided
    //  - the user did NOT pass --just-compile
    //  - it's not a non-compile command (--init)
    if let Some(ref dir) = cli.directory {
        if cli.just_compile && cli.init.is_none() {
            return run_native_just_compile(&cli, dir);
        }
        if !cli.just_compile && cli.init.is_none() {
            return compile_and_serve(dir, &cli);
        }
    }

    eprintln!("fastled: no sketch directory specified");
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::{Mutex, OnceLock};

    fn cwd_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct CurrentDirGuard {
        original: PathBuf,
    }

    impl CurrentDirGuard {
        fn enter(path: &Path) -> Self {
            let original = std::env::current_dir().expect("current dir");
            std::env::set_current_dir(path).expect("set current dir");
            Self { original }
        }
    }

    impl Drop for CurrentDirGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.original);
        }
    }

    fn base_cli() -> Cli {
        Cli {
            directory: None,
            serve_dir: None,
            init: None,
            just_compile: false,
            profile: false,
            install: false,
            dry_run: false,
            no_interactive: false,
            no_https: false,
            latest: false,
            branch: None,
            commit: None,
            fastled_path: None,
            purge: false,
            internal_ensure_fastled_repo: None,
            debug: false,
            quick: false,
            release: false,
        }
    }

    #[test]
    fn resolve_compile_directory_uses_current_sketch_dir() {
        let _lock = cwd_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp = tempfile::tempdir().expect("tempdir");
        let sketch_dir = temp.path().join("Blink");
        fs::create_dir_all(&sketch_dir).unwrap();
        fs::write(sketch_dir.join("Blink.ino"), b"void setup() {}").unwrap();
        let _guard = CurrentDirGuard::enter(&sketch_dir);

        let resolved = resolve_compile_directory(&base_cli()).unwrap();
        assert_eq!(resolved, Some(display_path(&sketch_dir)));
    }

    #[test]
    fn resolve_compile_directory_accepts_file_inside_sketch() {
        let temp = tempfile::tempdir().expect("tempdir");
        let sketch_dir = temp.path().join("Blink");
        let source_file = sketch_dir.join("Blink.ino");
        fs::create_dir_all(&sketch_dir).unwrap();
        fs::write(&source_file, b"void setup() {}").unwrap();

        let mut cli = base_cli();
        cli.directory = Some(display_path(&source_file));

        let resolved = resolve_compile_directory(&cli).unwrap();
        assert_eq!(resolved, Some(display_path(&sketch_dir)));
    }

    #[test]
    fn resolve_compile_directory_matches_partial_name() {
        let _lock = cwd_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp = tempfile::tempdir().expect("tempdir");
        let examples_dir = temp.path().join("examples").join("FxWave2d");
        fs::create_dir_all(&examples_dir).unwrap();
        fs::write(examples_dir.join("FxWave2d.ino"), b"void setup() {}").unwrap();
        let _guard = CurrentDirGuard::enter(temp.path());

        let mut cli = base_cli();
        cli.directory = Some("FxWave2d".to_string());

        let resolved = resolve_compile_directory(&cli).unwrap();
        assert_eq!(
            resolved,
            Some(
                PathBuf::from("examples")
                    .join("FxWave2d")
                    .to_string_lossy()
                    .into_owned()
            )
        );
    }

    #[test]
    fn detect_local_fastled_path_finds_repo_upwards() {
        let _lock = cwd_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp = tempfile::tempdir().expect("tempdir");
        let repo_root = temp.path().join("FastLED");
        let nested = repo_root.join("examples").join("Blink");
        fs::create_dir_all(&nested).unwrap();
        fs::write(repo_root.join("library.properties"), b"name=FastLED\n").unwrap();
        let _guard = CurrentDirGuard::enter(&nested);

        let detected = detect_local_fastled_path();
        assert_eq!(detected, Some(display_path(&repo_root)));
    }

    #[test]
    fn resolve_prompt_choice_accepts_default_selection() {
        let options = vec!["Blink".to_string(), "Noise".to_string()];
        match resolve_prompt_choice("", &options, 1) {
            PromptChoice::Selected(choice) => assert_eq!(choice, "Noise"),
            _ => panic!("expected default selection"),
        }
    }

    #[test]
    fn resolve_prompt_choice_accepts_numeric_selection() {
        let options = vec!["Blink".to_string(), "Noise".to_string()];
        match resolve_prompt_choice("2", &options, 0) {
            PromptChoice::Selected(choice) => assert_eq!(choice, "Noise"),
            _ => panic!("expected numeric selection"),
        }
    }

    #[test]
    fn resolve_prompt_choice_narrows_partial_matches() {
        let options = vec![
            "Fire2012".to_string(),
            "Fire2012WithPalette".to_string(),
            "Blink".to_string(),
        ];
        match resolve_prompt_choice("Fire", &options, 0) {
            PromptChoice::Narrowed(matches) => {
                assert_eq!(
                    matches,
                    vec!["Fire2012".to_string(), "Fire2012WithPalette".to_string()]
                );
            }
            _ => panic!("expected narrowed partial matches"),
        }
    }

    #[test]
    fn resolve_prompt_choice_uses_fuzzy_matching() {
        let options = vec![
            "examples/Audio".to_string(),
            "examples/BeatDetection".to_string(),
            "examples/Blink".to_string(),
        ];
        match resolve_prompt_choice("beats", &options, 0) {
            PromptChoice::Selected(choice) => {
                assert_eq!(choice, "examples/BeatDetection");
            }
            _ => panic!("expected fuzzy selection"),
        }
    }

    #[test]
    fn prepare_sketch_selection_auto_selects_single_directory() {
        let result = prepare_sketch_selection(vec![PathBuf::from("sketch1")], false, false);
        assert_eq!(result, SketchSelection::Selected("sketch1".to_string()));
    }

    #[test]
    fn prepare_sketch_selection_defers_large_first_scan() {
        let sketches = (0..5)
            .map(|index| PathBuf::from(format!("sketch{index}")))
            .collect();
        let result = prepare_sketch_selection(sketches, false, false);
        assert_eq!(result, SketchSelection::None);
    }

    #[test]
    fn prepare_sketch_selection_prompts_large_followup_scan() {
        let sketches: Vec<PathBuf> = (0..5)
            .map(|index| PathBuf::from(format!("sketch{index}")))
            .collect();
        let result = prepare_sketch_selection(sketches, false, true);
        assert_eq!(
            result,
            SketchSelection::Prompt(
                (0..5)
                    .map(|index| format!("sketch{index}"))
                    .collect::<Vec<_>>()
            )
        );
    }

    #[test]
    fn prepare_sketch_selection_filters_fastled_repo_dirs() {
        let sketches = vec![
            PathBuf::from("src"),
            PathBuf::from("dev"),
            PathBuf::from("tests"),
            PathBuf::from("examples"),
        ];
        let result = prepare_sketch_selection(sketches, true, false);
        assert_eq!(result, SketchSelection::Selected("examples".to_string()));
    }
}
