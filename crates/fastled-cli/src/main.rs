use clap::Parser;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

mod archive;
mod build;
mod keyboard;
mod project;
mod server;
mod viewer;
mod watcher;

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

/// Run `python -m fastled.app --just-compile` with piped stdout/stderr.
///
/// Each line is printed to the terminal AND sent through the broadcast
/// channel so the loading page receives it via SSE.
fn run_python_compile_streaming(
    cli: &Cli,
    extra_args: &[&str],
    tx: &tokio::sync::broadcast::Sender<String>,
) -> bool {
    let python = find_python();
    let mut args = vec![
        "-m".to_string(),
        "fastled.app".to_string(),
        "--just-compile".to_string(),
    ];

    if let Some(dir) = &cli.directory {
        args.push(dir.clone());
    }
    if cli.debug {
        args.push("--debug".to_string());
    } else if cli.release {
        args.push("--release".to_string());
    }
    if cli.profile {
        args.push("--profile".to_string());
    }
    if let Some(fp) = &cli.fastled_path {
        args.push("--fastled-path".to_string());
        args.push(fp.clone());
    }
    for arg in extra_args {
        args.push(arg.to_string());
    }

    let mut child = match Command::new(&python)
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("fastled: failed to launch `{python} -m fastled.app`: {e}");
            return false;
        }
    };

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let tx_out = tx.clone();
    let stdout_thread = std::thread::spawn(move || {
        let reader = std::io::BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            println!("{line}");
            send_sse(
                &tx_out,
                &format!(
                    r#"{{"type":"log","line":"{}","stream":"stdout"}}"#,
                    json_escape(&line)
                ),
            );
        }
    });

    let tx_err = tx.clone();
    let stderr_thread = std::thread::spawn(move || {
        let reader = std::io::BufReader::new(stderr);
        for line in reader.lines().map_while(Result::ok) {
            eprintln!("{line}");
            send_sse(
                &tx_err,
                &format!(
                    r#"{{"type":"log","line":"{}","stream":"stderr"}}"#,
                    json_escape(&line)
                ),
            );
        }
    });

    stdout_thread.join().ok();
    stderr_thread.join().ok();

    match child.wait() {
        Ok(s) => s.success(),
        Err(e) => {
            eprintln!("fastled: error waiting for subprocess: {e}");
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Viewer mode selector
// ---------------------------------------------------------------------------

/// Controls how the compiled output is displayed to the user.
enum ViewerMode {
    /// Open the default system browser.
    Browser,
    /// Launch the native Tauri viewer pointing at the output directory.
    TauriViewer,
}

// ---------------------------------------------------------------------------
// Compile + serve + watch (replaces Flask-based flow)
// ---------------------------------------------------------------------------

/// Compile a sketch, serve the output via the built-in HTTP server, and
/// watch for file changes to trigger recompilation.
///
/// Build output is streamed to the browser (or Tauri viewer) in real time
/// via SSE.
fn compile_and_serve(dir: &str, cli: &Cli, mode: ViewerMode) -> ExitCode {
    let sketch_dir = PathBuf::from(dir);
    if !sketch_dir.is_dir() {
        eprintln!("fastled: sketch directory does not exist: {dir}");
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

        match mode {
            ViewerMode::Browser => open_browser(&url),
            ViewerMode::TauriViewer => {
                if let Err(e) = viewer::launch_tauri_viewer(&output_dir) {
                    eprintln!("fastled: Tauri viewer failed: {e}, falling back to browser");
                    open_browser(&url);
                }
            }
        }

        // --- Initial compilation ------------------------------------------------
        send_sse(
            &tx,
            r#"{"type":"status","status":"compiling","message":"Compiling..."}"#,
        );

        let mut extra: Vec<&str> = Vec::new();
        if cli.purge {
            extra.push("--purge");
        }
        let success = run_python_compile_streaming(cli, &extra, &tx);

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

                let success = run_python_compile_streaming(cli, &[], &tx);
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
/// Thin Rust front-end that mirrors every Python flag, then delegates to
/// `python -m fastled.app` so behaviour is identical while the Rust entry
/// point is established.
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

    /// Use Playwright app-like browser experience (downloads browsers if needed).
    #[arg(long)]
    app: bool,

    /// Force the legacy Flask + browser viewer even when the native Tauri
    /// viewer is available.  Has no effect unless `--app` is also passed.
    #[arg(long)]
    legacy_browser: bool,

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

/// Locate a Python executable to use.
///
/// Priority:
///  1. `VIRTUAL_ENV/Scripts/python.exe` (Windows) or `VIRTUAL_ENV/bin/python` (Unix)
///  2. Plain `python` on PATH
///  3. Plain `python3` on PATH
fn find_python() -> String {
    if let Ok(venv) = std::env::var("VIRTUAL_ENV") {
        #[cfg(windows)]
        let candidate = format!("{}/Scripts/python.exe", venv);
        #[cfg(not(windows))]
        let candidate = format!("{}/bin/python", venv);

        if std::path::Path::new(&candidate).exists() {
            return candidate;
        }
    }

    // Try plain `python`
    if Command::new("python")
        .args(["--version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        return "python".to_string();
    }

    // Fall back to `python3`
    "python3".to_string()
}

/// Decide whether to use the native Tauri viewer for this invocation.
///
/// Returns `true` when:
/// * `--app` was requested, AND
/// * `--legacy-browser` was NOT passed, AND
/// * the `fastled-viewer` binary can be found.
fn should_use_tauri_viewer(cli: &Cli) -> bool {
    cli.app && !cli.legacy_browser && viewer::viewer_available()
}

/// Convert the parsed `Cli` struct back into the argv that the Python CLI
/// expects, so we can pass it through verbatim.
///
/// This is used only for the delegation path (--just-compile, --init,
/// --install) where Python handles the full operation.
fn rebuild_python_args(cli: &Cli) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();

    if let Some(dir) = &cli.directory {
        args.push(dir.clone());
    }
    if let Some(sd) = &cli.serve_dir {
        args.push("--serve-dir".to_string());
        args.push(sd.clone());
    }
    if let Some(init_val) = &cli.init {
        args.push("--init".to_string());
        // "__init__" is the sentinel used for bare `--init` (no value)
        if init_val != "__init__" {
            args.push(init_val.clone());
        }
    }
    if cli.just_compile {
        args.push("--just-compile".to_string());
    }
    if cli.profile {
        args.push("--profile".to_string());
    }
    if cli.app {
        args.push("--app".to_string());
    }
    if cli.install {
        args.push("--install".to_string());
    }
    if cli.dry_run {
        args.push("--dry-run".to_string());
    }
    if cli.no_interactive {
        args.push("--no-interactive".to_string());
    }
    if cli.no_https {
        args.push("--no-https".to_string());
    }
    if cli.latest {
        args.push("--latest".to_string());
    }
    if let Some(branch) = &cli.branch {
        args.push("--branch".to_string());
        args.push(branch.clone());
    }
    if let Some(commit) = &cli.commit {
        args.push("--commit".to_string());
        args.push(commit.clone());
    }
    if let Some(fp) = &cli.fastled_path {
        args.push("--fastled-path".to_string());
        args.push(fp.clone());
    }
    if cli.purge {
        args.push("--purge".to_string());
    }
    if cli.debug {
        args.push("--debug".to_string());
    }
    if cli.quick {
        args.push("--quick".to_string());
    }
    if cli.release {
        args.push("--release".to_string());
    }

    args
}

/// Serve a directory using the built-in Rust HTTP server.
///
/// This replaces the Flask-based `--serve-dir` implementation with a native
/// Rust server, eliminating the Python/Flask dependency for this code path.
fn serve_directory(dir: &str, cli: &Cli) -> ExitCode {
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

        // Open the Tauri viewer or fall back to system browser.
        if viewer::viewer_available() && !cli.no_https {
            if let Err(e) = viewer::launch_tauri_viewer(&path) {
                eprintln!("fastled: Tauri viewer failed: {e}, opening system browser");
                open_browser(&url);
            }
        } else {
            open_browser(&url);
        }

        // Wait for Ctrl+C.
        tokio::signal::ctrl_c().await.ok();
        println!("\nShutting down...");
        ExitCode::SUCCESS
    })
}

/// Open a URL in the default system browser.
fn open_browser(url: &str) {
    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("cmd").args(["/c", "start", url]).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open").arg(url).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = Command::new("xdg-open").arg(url).spawn();
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    // Handle --serve-dir natively with the Rust HTTP server (no Python needed).
    if let Some(ref serve_dir) = cli.serve_dir {
        return serve_directory(serve_dir, &cli);
    }

    let use_tauri = should_use_tauri_viewer(&cli);

    // Normal compilation flow with Rust HTTP server + watch mode.
    // Both browser and Tauri viewer paths use the same compile_and_serve()
    // infrastructure so compilation output is streamed via SSE in real time.
    //
    // Conditions:
    //  - a sketch directory was provided
    //  - the user did NOT pass --just-compile
    //  - it's not a non-compile command (--init, --install)
    if let Some(ref dir) = cli.directory {
        if !cli.just_compile && cli.init.is_none() && !cli.install {
            let mode = if use_tauri {
                ViewerMode::TauriViewer
            } else {
                ViewerMode::Browser
            };
            return compile_and_serve(dir, &cli, mode);
        }
    }

    // --- Delegate to Python for everything else ------------------------------
    // (--just-compile, --init, --install, etc.)

    let python = find_python();
    let py_args = rebuild_python_args(&cli);

    let status = Command::new(&python)
        .args(["-m", "fastled.app"])
        .args(&py_args)
        .status();

    let exit_code = match status {
        Ok(s) => {
            let code = s.code().unwrap_or(1);
            ExitCode::from(code as u8)
        }
        Err(e) => {
            eprintln!("fastled: failed to launch `{python} -m fastled.app`: {e}");
            return ExitCode::FAILURE;
        }
    };

    exit_code
}
