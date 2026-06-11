use std::process::ExitCode;

pub struct ViewerOptions {
    /// HTTP URL of the embedded fastled server (e.g. `http://127.0.0.1:8089/`).
    ///
    /// The viewer must load the server URL — not files off disk — so the
    /// loading page, SSE build stream, and post-build reload all work while
    /// `index.html` does not exist yet (issue #151).
    pub url: String,
    pub title: String,
    pub width: u32,
    pub height: u32,
}

pub fn run(options: ViewerOptions) -> ExitCode {
    let title = options.title;
    let width = options.width;
    let height = options.height;
    let url = options.url;

    let result = tauri::Builder::default()
        .setup(move |app| {
            let url = tauri::WebviewUrl::External(url.parse()?);

            let window = tauri::WebviewWindowBuilder::new(app, "main", url)
                .title(&title)
                .inner_size(width as f64, height as f64)
                .build()?;

            // Counteract WebView2 DPI auto-scaling while keeping the UI readable.
            let scale = window.scale_factor().unwrap_or(1.0);
            if scale > 1.0 {
                let zoom = 0.92 / scale;
                let js = format!(
                    "document.addEventListener('DOMContentLoaded', function() {{ document.body.style.zoom = '{}'; }});",
                    zoom
                );
                window.eval(&js).ok();
            }
            Ok(())
        })
        .run(tauri::generate_context!());

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("fastled: Tauri viewer failed: {err}");
            ExitCode::FAILURE
        }
    }
}
