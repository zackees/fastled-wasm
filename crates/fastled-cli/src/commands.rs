use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, RwLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::build;
use crate::cli::{requested_init_ref, validate_init_ref_flags, Cli, LinkMode};
use crate::compile_stream::{
    announce_link_mode, ensure_compile_prerequisites, purge_fastled_cache, report_build_outcome,
    run_native_compile_streaming, selected_build_mode, send_sse, write_build_status,
};
use crate::debug_symbols;
use crate::dwarf_smoke::run_dwarf_source_smoke;
use crate::dynamic_cache;
use crate::install;
use crate::keyboard;
use crate::path::NormalizedPath;
use crate::project;
use crate::selection::prompt_for_example;
use crate::server;
use crate::test_mode::{self, TestOutcome};
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

    announce_link_mode(cli, None, true);

    // Ensure the emscripten + esbuild toolchains are installed before invoking
    // the native Rust build backend. The backend consumes the Rust-installed
    // directories via these environment variables.
    if let Err(message) = ensure_compile_prerequisites(true) {
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
            None,
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

        let mut viewer = match viewer::launch_tauri_viewer(&url) {
            Ok(process) => process,
            Err(e) => {
                eprintln!("fastled: Tauri viewer failed: {e:#}");
                return ExitCode::FAILURE;
            }
        };

        // --- Initial compilation ------------------------------------------------
        // This process stays alive for rebuilds. Keep authoritative startup
        // fingerprints in memory and invalidate them from filesystem events.
        std::env::set_var("FASTLED_PERSISTENT_FINGERPRINTS", "1");
        send_sse(
            &tx,
            r#"{"type":"status","status":"compiling","message":"Compiling..."}"#,
        );
        announce_link_mode(cli, Some(&tx), false);

        let success =
            run_native_compile_streaming(cli, &sketch_dir, cli.purge, &tx, &debug_symbols);
        report_build_outcome(&output_dir, &tx, success, !cli.no_app);

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
            // The viewer window is the app from the user's perspective: once
            // it is gone (closed or crashed), shut the CLI down cleanly. An
            // in-flight compile always finishes first because this check only
            // runs between loop ticks.
            if !viewer.is_alive() {
                println!("\nViewer window closed; shutting down.");
                break;
            }

            let should_rebuild = match rx.recv_timeout(std::time::Duration::from_secs(1)) {
                Ok(batch) => {
                    println!(
                        "\nChanges detected in {:?}{}",
                        batch.paths,
                        if batch.force_rescan {
                            " (full rescan required)"
                        } else {
                            ""
                        }
                    );
                    let changed_paths = batch
                        .paths
                        .iter()
                        .map(NormalizedPath::new)
                        .collect::<Vec<_>>();
                    let invalidation = if batch.force_rescan {
                        dynamic_cache::invalidate_all_persistent_fingerprints()
                    } else {
                        dynamic_cache::invalidate_persistent_fingerprints(&changed_paths)
                    };
                    if let Err(error) = invalidation {
                        eprintln!("fastled: persistent fingerprint invalidation failed; falling back to full scans: {error:#}");
                        std::env::remove_var("FASTLED_PERSISTENT_FINGERPRINTS");
                    }
                    true
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    let manual = keyboard::check_for_space();
                    if manual {
                        if let Err(error) = dynamic_cache::invalidate_all_persistent_fingerprints() {
                            eprintln!("fastled: persistent fingerprint invalidation failed; falling back to full scans: {error:#}");
                            std::env::remove_var("FASTLED_PERSISTENT_FINGERPRINTS");
                        }
                    }
                    manual
                }
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
                if report_build_outcome(&output_dir, &tx, success, !cli.no_app) {
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

/// Compile once, render in the shipped Tauri viewer, collect deterministic
/// test artifacts, and tear the contained viewer/server processes down.
pub(crate) fn compile_and_test(dir: &str, cli: &Cli) -> ExitCode {
    let started = Instant::now();
    let plan = match test_mode::build_test_plan(cli) {
        Ok(plan) => plan,
        Err(message) => {
            eprintln!("fastled: {message}");
            return test_exit(TestOutcome::Failure);
        }
    };
    let sketch_dir = PathBuf::from(dir);
    if !sketch_dir.is_dir() {
        eprintln!("fastled: sketch directory does not exist: {dir}");
        return test_exit(TestOutcome::Failure);
    }

    announce_link_mode(cli, None, true);

    let output_dir = sketch_dir.join("fastled_js");
    if let Err(error) = std::fs::create_dir_all(&output_dir) {
        eprintln!(
            "fastled: could not create output directory {}: {error}",
            output_dir.display()
        );
        return test_exit(TestOutcome::Failure);
    }

    let mut log_file = match open_test_log(plan.log_path.as_deref()) {
        Ok(file) => file,
        Err(error) => {
            eprintln!("fastled: {error}");
            return test_exit(TestOutcome::Failure);
        }
    };
    let (build_tx, _build_rx) = tokio::sync::broadcast::channel::<String>(256);
    let (test_tx, mut test_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut token_bytes = [0_u8; 32];
    if let Err(error) = getrandom::fill(&mut token_bytes) {
        eprintln!("fastled: could not create test capability: {error}");
        return test_exit(TestOutcome::Failure);
    }
    let test_token = token_bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let debug_symbols: server::DebugSymbolHandle = Arc::new(RwLock::new(None));
    let screenshot_paths = plan
        .screenshots
        .iter()
        .map(|(name, path)| (name.clone(), path.clone().into_path_buf()))
        .collect::<HashMap<_, _>>();
    let runtime_config = server::TestRuntimeConfig {
        wait_ms: plan.wait.as_secs_f64() * 1_000.0,
        interval_ms: plan.interval.map(|value| value.as_secs_f64() * 1_000.0),
        screenshot_names: plan
            .screenshots
            .iter()
            .map(|(name, _)| name.clone())
            .collect(),
    };

    write_build_status(&output_dir, "compiling", "Compiling...");
    let rt = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("fastled: could not create test runtime: {error}");
            return test_exit(TestOutcome::Failure);
        }
    };
    rt.block_on(async {
        let addr = match server::start_server(
            output_dir.clone(),
            0,
            Some(build_tx.clone()),
            debug_symbols.clone(),
            Some(server::TestServerOptions {
                runtime: runtime_config,
                screenshot_paths,
                events: test_tx,
                token: test_token.clone(),
                sleep_permits: Arc::new(tokio::sync::Semaphore::new(4)),
            }),
        )
        .await
        {
            Ok(addr) => addr,
            Err(error) => {
                eprintln!("fastled: failed to start test server: {error}");
                return test_exit(TestOutcome::Failure);
            }
        };

        send_sse(
            &build_tx,
            r#"{"type":"status","status":"compiling","message":"Compiling..."}"#,
        );
        announce_link_mode(cli, Some(&build_tx), false);
        let Some(compile_budget) = plan.total_timeout.checked_sub(started.elapsed()) else {
            eprintln!("fastled: production test timed out before compilation");
            return test_exit(TestOutcome::TotalTimeout);
        };
        let mut compile = match test_compile_command(cli, &sketch_dir) {
            Ok(command) => command,
            Err(error) => {
                eprintln!("fastled: could not launch test compilation: {error}");
                return test_exit(TestOutcome::Failure);
            }
        };
        let compile_result = match test_mode::run_contained_command(&mut compile, compile_budget).await
        {
            Ok(result) => result,
            Err(error) => {
                eprintln!("fastled: test compilation process failed: {error}");
                return test_exit(TestOutcome::Failure);
            }
        };
        let success = matches!(compile_result, test_mode::TimedCommandResult::Exited(0));
        let effective_success = report_build_outcome(&output_dir, &build_tx, success, true);
        if matches!(compile_result, test_mode::TimedCommandResult::Interrupted) {
            eprintln!("fastled: production test interrupted during compilation");
            return test_exit(TestOutcome::Interrupted);
        }
        if matches!(compile_result, test_mode::TimedCommandResult::TimedOut) {
            eprintln!("fastled: production test exceeded --test-timeout-secs during compilation");
            return test_exit(TestOutcome::TotalTimeout);
        }
        if !effective_success {
            eprintln!("fastled: test compilation failed");
            return test_exit(TestOutcome::Failure);
        }

        let url = format!("http://{addr}/#fastled-test-token={test_token}");
        let mut viewer = match viewer::launch_tauri_test_viewer(&url) {
            Ok(process) => process,
            Err(error) => {
                eprintln!("fastled: Tauri test viewer failed: {error:#}");
                return test_exit(TestOutcome::Failure);
            }
        };

        let Some(remaining) = plan.total_timeout.checked_sub(started.elapsed()) else {
            eprintln!("fastled: production test timed out while launching the viewer");
            return test_exit(TestOutcome::TotalTimeout);
        };
        let deadline_origin = tokio::time::Instant::now();
        let total_deadline = deadline_origin + remaining;
        let ready_deadline = deadline_origin + plan.ready_timeout;
        let ctrl_c = tokio::signal::ctrl_c();
        tokio::pin!(ctrl_c);
        let mut liveness = tokio::time::interval(std::time::Duration::from_millis(100));
        let mut ready = false;
        let mut page_error = false;
        let mut saved = HashSet::new();
        let mut viewer_done: Option<u8> = None;
        let mut commands_done = plan.commands.is_empty();
        let (command_tx, mut command_rx) = tokio::sync::mpsc::channel(256);
        let mut command_tx = Some(command_tx);
        let mut _command_task: Option<CommandTaskGuard> = None;

        loop {
            tokio::select! {
                biased;
                _ = tokio::time::sleep_until(total_deadline) => {
                    eprintln!("fastled: production test exceeded --test-timeout-secs");
                    return test_exit(TestOutcome::TotalTimeout);
                }
                _ = tokio::time::sleep_until(ready_deadline), if !ready => {
                    eprintln!("fastled: viewer did not render a canvas before --test-ready-timeout-secs");
                    return test_exit(TestOutcome::ReadyTimeout);
                }
                _ = &mut ctrl_c => {
                    eprintln!("fastled: production test interrupted");
                    return test_exit(TestOutcome::Interrupted);
                }
                _ = liveness.tick() => {
                    if !viewer.is_alive() {
                        eprintln!("fastled: test viewer exited before completing");
                        return test_exit(TestOutcome::Failure);
                    }
                }
                event = test_rx.recv() => {
                    match event {
                        Some(server::TestEvent::Ready) => {
                            if !ready {
                                ready = true;
                                if !write_test_log(&mut log_file, "[fastled-test] ready") {
                                    return test_exit(TestOutcome::Failure);
                                }
                                if !plan.commands.is_empty() {
                                    let Some(command_sender) = command_tx.take() else {
                                        eprintln!("fastled: command runner was already started");
                                        return test_exit(TestOutcome::Failure);
                                    };
                                    _command_task = Some(CommandTaskGuard(tokio::spawn(
                                        test_mode::run_test_commands(
                                            plan.commands.clone(),
                                            NormalizedPath::new(&sketch_dir),
                                            command_sender,
                                        ),
                                    )));
                                }
                            }
                        }
                        Some(server::TestEvent::ViewerLog(line)) => {
                            page_error |= test_mode::is_viewer_error_line(&line);
                            if !write_test_log(&mut log_file, &line) {
                                return test_exit(TestOutcome::Failure);
                            }
                        }
                        Some(server::TestEvent::ScreenshotSaved { name, path }) => {
                            saved.insert(name);
                            let marker = format!("[fastled-test] screenshot={}", path.display());
                            if !write_test_log(&mut log_file, &marker) {
                                return test_exit(TestOutcome::Failure);
                            }
                        }
                        Some(server::TestEvent::Failure(message)) => {
                            eprintln!("fastled: {message}");
                            return test_exit(TestOutcome::Failure);
                        }
                        Some(server::TestEvent::Done(code)) => {
                            if !ready || code != 0 || saved.len() != plan.screenshots.len() {
                                eprintln!(
                                    "fastled: viewer test failed (ready={ready}, code {code}, saved {}/{})",
                                    saved.len(),
                                    plan.screenshots.len()
                                );
                                return test_exit(TestOutcome::Failure);
                            }
                            viewer_done = Some(code);
                            if commands_done {
                                return test_exit(if plan.exit_on_error && page_error {
                                    TestOutcome::PageError
                                } else { TestOutcome::Success });
                            }
                        }
                        None => {
                            eprintln!("fastled: test event channel closed unexpectedly");
                            return test_exit(TestOutcome::Failure);
                        }
                    }
                }
                command_event = command_rx.recv(), if !commands_done => {
                    match command_event {
                        Some(test_mode::TestCommandEvent::Start { index }) => {
                            let marker = format!("[fastled-test-cmd {index}] start");
                            println!("{marker}");
                            if !write_test_log(&mut log_file, &marker) { return test_exit(TestOutcome::Failure); }
                        }
                        Some(test_mode::TestCommandEvent::Output { index, stream, line }) => {
                            let stream_name = match stream { test_mode::CommandStream::Stdout => "stdout", test_mode::CommandStream::Stderr => "stderr" };
                            let marker = format!("[fastled-test-cmd {index} {stream_name}] {line}");
                            match stream { test_mode::CommandStream::Stdout => println!("{line}"), test_mode::CommandStream::Stderr => eprintln!("{line}") }
                            if !write_test_log(&mut log_file, &marker) { return test_exit(TestOutcome::Failure); }
                        }
                        Some(test_mode::TestCommandEvent::Exit { index, code }) => {
                            let marker = format!("[fastled-test-cmd {index}] exit={code}");
                            println!("{marker}");
                            if !write_test_log(&mut log_file, &marker) { return test_exit(TestOutcome::Failure); }
                        }
                        Some(test_mode::TestCommandEvent::Done(result)) => {
                            commands_done = true;
                            if let Err(message) = result {
                                eprintln!("fastled: {message}");
                                return test_exit(TestOutcome::Failure);
                            }
                            if viewer_done.is_some() {
                                return test_exit(if plan.exit_on_error && page_error { TestOutcome::PageError } else { TestOutcome::Success });
                            }
                        }
                        None => {
                            eprintln!("fastled: command task ended without a completion result");
                            return test_exit(TestOutcome::Failure);
                        }
                    }
                }
            }
        }
    })
}

struct CommandTaskGuard(tokio::task::JoinHandle<()>);

impl Drop for CommandTaskGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn test_exit(outcome: TestOutcome) -> ExitCode {
    ExitCode::from(outcome.exit_code())
}

fn test_compile_command(
    cli: &Cli,
    sketch_dir: &std::path::Path,
) -> std::io::Result<std::process::Command> {
    let mut command = std::process::Command::new(std::env::current_exe()?);
    command
        .arg(sketch_dir)
        .arg("--just-compile")
        .arg("--no-interactive");
    if cli.debug {
        command.arg("--debug");
    } else if cli.release {
        command.arg("--release");
    } else {
        command.arg("--quick");
    }
    command.arg(match cli.link_mode {
        LinkMode::Static => "--link=static",
        LinkMode::Dynamic => "--link=dynamic",
    });
    if cli.profile {
        command.arg("--profile");
    }
    if cli.purge {
        command.arg("--purge");
    }
    if cli.clangd {
        command.arg("--clangd");
    }
    if let Some(path) = &cli.fastled_path {
        command.arg("--fastled-path").arg(path);
    }
    Ok(command)
}

fn open_test_log(path: Option<&std::path::Path>) -> Result<Option<File>, String> {
    let Some(path) = path else {
        return Ok(None);
    };
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("could not create test log directory: {error}"))?;
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map(Some)
        .map_err(|error| format!("could not open test log {}: {error}", path.display()))
}

fn write_test_log(file: &mut Option<File>, line: &str) -> bool {
    let Some(file) = file else {
        return true;
    };
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    if let Err(error) = writeln!(file, "[{timestamp}] {line}").and_then(|_| file.flush()) {
        eprintln!("fastled: could not write test log: {error}");
        return false;
    }
    true
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
    announce_link_mode(cli, None, true);
    if let Err(message) = ensure_compile_prerequisites(!cli.no_app) {
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
        emit_clangd: cli.clangd,
        no_app: cli.no_app,
        link_mode: crate::compile_stream::effective_link_mode(cli),
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
        let addr = match server::start_server(path.clone(), 0, None, debug_symbols, None).await {
            Ok(a) => a,
            Err(e) => {
                eprintln!("fastled: failed to start server: {e}");
                return ExitCode::FAILURE;
            }
        };

        let url = format!("http://{addr}");
        println!("Serving {dir} at {url}");
        println!("Press Ctrl+C to stop...");

        let viewer = if launch_viewer {
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

        match viewer {
            Some(mut viewer) => {
                // Exit on Ctrl+C or when the viewer window is closed.
                let ctrl_c = tokio::signal::ctrl_c();
                tokio::pin!(ctrl_c);
                loop {
                    tokio::select! {
                        _ = &mut ctrl_c => {
                            println!("\nShutting down...");
                            break;
                        }
                        _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {
                            if !viewer.is_alive() {
                                println!("\nViewer window closed; shutting down.");
                                break;
                            }
                        }
                    }
                }
            }
            None => {
                // Headless serve has no viewer to watch; run until Ctrl+C.
                tokio::signal::ctrl_c().await.ok();
                println!("\nShutting down...");
            }
        }
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
    if let Err(message) = ensure_compile_prerequisites(true) {
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
        emit_clangd: cli.clangd,
        no_app: false,
        link_mode: crate::cli::LinkMode::Static,
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
