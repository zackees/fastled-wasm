use clap::Parser;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

mod archive;
mod build;
mod keyboard;
mod project;
mod server;
mod viewer;
mod watcher;

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
/// When `tauri_viewer` is `true` the Python process is asked only to compile
/// (via `--just-compile`); the Rust caller will subsequently launch the Tauri
/// viewer instead of delegating browser launching to Python.
fn rebuild_python_args(cli: &Cli, tauri_viewer: bool) -> Vec<String> {
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
    // When Tauri viewer is handling display, tell Python to only compile.
    if cli.just_compile || tauri_viewer {
        args.push("--just-compile".to_string());
    }
    if cli.profile {
        args.push("--profile".to_string());
    }
    // Pass --app through only when we are NOT taking over with the Tauri viewer.
    if cli.app && !tauri_viewer {
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

/// Derive the expected build output directory from CLI arguments.
///
/// Mirrors `BuildRequest::output_dir()` in `build.rs`: the Python CLI always
/// writes compiled artefacts to `<sketch_dir>/fastled_js`.
fn output_dir_from_cli(cli: &Cli) -> Option<PathBuf> {
    cli.directory
        .as_ref()
        .map(|d| PathBuf::from(d).join("fastled_js"))
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let use_tauri = should_use_tauri_viewer(&cli);

    if use_tauri {
        eprintln!("fastled: native Tauri viewer detected — compiling then launching viewer");
    }

    let python = find_python();
    let py_args = rebuild_python_args(&cli, use_tauri);

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

    // If we are driving the Tauri viewer and the Python compile step succeeded,
    // launch the native viewer pointing at the build output directory.
    if use_tauri && exit_code == ExitCode::SUCCESS {
        if let Some(out_dir) = output_dir_from_cli(&cli) {
            match viewer::launch_tauri_viewer(&out_dir) {
                Ok(mut child) => {
                    // Wait for the viewer to exit so the terminal session stays
                    // alive until the user closes the window.
                    let viewer_status = child.wait();
                    match viewer_status {
                        Ok(s) => {
                            let code = s.code().unwrap_or(1);
                            return ExitCode::from(code as u8);
                        }
                        Err(e) => {
                            eprintln!("fastled: error waiting for viewer: {e}");
                            return ExitCode::FAILURE;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("fastled: failed to launch Tauri viewer: {e}");
                    eprintln!("fastled: falling back to legacy Flask browser");
                    // Re-run Python without --just-compile to use Flask fallback.
                    let fallback_args = rebuild_python_args(&cli, false);
                    let fallback_status = Command::new(&python)
                        .args(["-m", "fastled.app"])
                        .args(&fallback_args)
                        .status();
                    return match fallback_status {
                        Ok(s) => ExitCode::from(s.code().unwrap_or(1) as u8),
                        Err(e2) => {
                            eprintln!(
                                "fastled: fallback also failed to launch `{python} -m fastled.app`: {e2}"
                            );
                            ExitCode::FAILURE
                        }
                    };
                }
            }
        } else {
            eprintln!("fastled: --app with Tauri viewer requires a sketch directory argument");
            return ExitCode::FAILURE;
        }
    }

    exit_code
}
