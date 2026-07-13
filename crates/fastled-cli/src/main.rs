use std::process::ExitCode;

#[cfg(feature = "viewer")]
mod tauri_viewer;

struct InternalViewerArgs {
    url: String,
    title: String,
    width: u32,
    height: u32,
    inject_test_runtime: bool,
}

fn parse_internal_viewer_args() -> Option<Result<InternalViewerArgs, String>> {
    let mut args = std::env::args_os().skip(1);
    let first = args.next()?;
    if first != "--internal-viewer" {
        return None;
    }

    let Some(url) = args.next() else {
        return Some(Err("--internal-viewer requires a URL".to_string()));
    };

    let mut parsed = InternalViewerArgs {
        url: url.to_string_lossy().into_owned(),
        title: "FastLED Viewer".to_string(),
        width: 800,
        height: 600,
        inject_test_runtime: false,
    };

    while let Some(arg) = args.next() {
        if arg == "--viewer-title" {
            let Some(value) = args.next() else {
                return Some(Err("--viewer-title requires a value".to_string()));
            };
            parsed.title = value.to_string_lossy().into_owned();
        } else if arg == "--viewer-width" {
            let Some(value) = args.next() else {
                return Some(Err("--viewer-width requires a value".to_string()));
            };
            let value = value.to_string_lossy();
            parsed.width = match value.parse::<u32>() {
                Ok(width) => width,
                Err(_) => return Some(Err("--viewer-width must be an integer".to_string())),
            };
        } else if arg == "--viewer-height" {
            let Some(value) = args.next() else {
                return Some(Err("--viewer-height requires a value".to_string()));
            };
            let value = value.to_string_lossy();
            parsed.height = match value.parse::<u32>() {
                Ok(height) => height,
                Err(_) => return Some(Err("--viewer-height must be an integer".to_string())),
            };
        } else if arg == "--viewer-inject-test-runtime" {
            parsed.inject_test_runtime = true;
        } else {
            return Some(Err(format!(
                "unknown internal viewer argument: {}",
                arg.to_string_lossy()
            )));
        }
    }

    Some(Ok(parsed))
}

fn run_internal_viewer(args: InternalViewerArgs) -> ExitCode {
    if !args.url.starts_with("http://") && !args.url.starts_with("https://") {
        eprintln!(
            "fastled: --internal-viewer requires an http(s) URL, got: {}",
            args.url
        );
        return ExitCode::FAILURE;
    }

    #[cfg(feature = "viewer")]
    {
        tauri_viewer::run(tauri_viewer::ViewerOptions {
            url: args.url,
            title: args.title,
            width: args.width,
            height: args.height,
            inject_test_runtime: args.inject_test_runtime,
        })
    }

    #[cfg(not(feature = "viewer"))]
    {
        eprintln!("fastled: this fastled binary was built without viewer support");
        ExitCode::FAILURE
    }
}

fn exit_code_from_process(code: i32) -> ExitCode {
    if code == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from((code & 0xff) as u8)
    }
}

fn main() -> ExitCode {
    let raw_args = std::env::args_os().collect::<Vec<_>>();
    match fastled_cli::runtime::maybe_reexec_from_managed_runtime(&raw_args) {
        Ok(Some(code)) => return exit_code_from_process(code),
        Ok(None) => {}
        Err(err) => {
            eprintln!("fastled: runtime handoff failed: {err:#}");
        }
    }

    match parse_internal_viewer_args() {
        Some(Ok(args)) => run_internal_viewer(args),
        Some(Err(message)) => {
            eprintln!("fastled: {message}");
            ExitCode::FAILURE
        }
        None => fastled_cli::run(),
    }
}
