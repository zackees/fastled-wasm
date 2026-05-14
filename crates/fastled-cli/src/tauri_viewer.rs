use std::path::PathBuf;
use std::process::ExitCode;

pub struct ViewerOptions {
    pub frontend_dir: PathBuf,
    pub title: String,
    pub width: u32,
    pub height: u32,
}

pub fn run(options: ViewerOptions) -> ExitCode {
    let mut builder = tauri::Builder::default();
    let serve_dir = options.frontend_dir.clone();

    builder = builder.register_asynchronous_uri_scheme_protocol(
        "fastled",
        move |_ctx, request, responder| {
            let path = request.uri().path();
            let relative = path.trim_start_matches('/');
            let relative = if relative.is_empty() {
                "index.html"
            } else {
                relative
            };
            let file_path = serve_dir.join(relative);

            let mime = match file_path.extension().and_then(|e| e.to_str()).unwrap_or("") {
                "html" => "text/html",
                "js" => "application/javascript",
                "wasm" => "application/wasm",
                "css" => "text/css",
                "json" => "application/json",
                "png" => "image/png",
                "svg" => "image/svg+xml",
                "ico" => "image/x-icon",
                "ttf" | "otf" => "font/ttf",
                "woff" => "font/woff",
                "woff2" => "font/woff2",
                "map" => "application/json",
                _ => "application/octet-stream",
            };

            match std::fs::read(&file_path) {
                Ok(data) => {
                    let response = tauri::http::Response::builder()
                        .status(200)
                        .header("Content-Type", mime)
                        .header("Cross-Origin-Opener-Policy", "same-origin")
                        .header("Cross-Origin-Embedder-Policy", "require-corp")
                        .body(data)
                        .unwrap();
                    responder.respond(response);
                }
                Err(_) => {
                    let response = tauri::http::Response::builder()
                        .status(404)
                        .body(b"Not Found".to_vec())
                        .unwrap();
                    responder.respond(response);
                }
            }
        },
    );

    let title = options.title;
    let width = options.width;
    let height = options.height;

    let result = builder
        .setup(move |app| {
            let url =
                tauri::WebviewUrl::CustomProtocol("fastled://localhost/index.html".parse()?);

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
