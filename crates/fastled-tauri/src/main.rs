use std::path::PathBuf;

use clap::Parser;

#[derive(Parser)]
#[command(name = "fastled-viewer", version, about = "FastLED WASM native viewer")]
struct Cli {
    /// Directory containing compiled frontend assets (fastled.js, fastled.wasm, index.html)
    #[arg(long)]
    frontend_dir: Option<String>,

    /// Window title
    #[arg(long, default_value = "FastLED Viewer")]
    title: String,

    /// Window width
    #[arg(long, default_value = "800")]
    width: u32,

    /// Window height
    #[arg(long, default_value = "600")]
    height: u32,
}

fn main() {
    let cli = Cli::parse();

    let frontend_dir: Option<PathBuf> = cli.frontend_dir.as_ref().map(PathBuf::from);

    let mut builder = tauri::Builder::default();

    // If a frontend directory is provided, register a custom protocol to serve
    // files from it instead of the bundled frontend assets.
    if let Some(ref dir) = frontend_dir {
        let serve_dir = dir.clone();
        builder = builder.register_asynchronous_uri_scheme_protocol(
            "fastled",
            move |_ctx, request, responder| {
                let path = request.uri().path();
                // Strip leading slash and default to index.html
                let relative = path.trim_start_matches('/');
                let relative = if relative.is_empty() {
                    "index.html"
                } else {
                    relative
                };
                let file_path = serve_dir.join(relative);

                let mime = match file_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                {
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
    }

    let title = cli.title.clone();
    let width = cli.width;
    let height = cli.height;
    let use_custom_protocol = frontend_dir.is_some();

    builder
        .setup(move |app| {
            let url = if use_custom_protocol {
                tauri::WebviewUrl::CustomProtocol("fastled://localhost/index.html".parse().unwrap())
            } else {
                tauri::WebviewUrl::App("index.html".into())
            };

            let window = tauri::WebviewWindowBuilder::new(app, "main", url)
                .title(&title)
                .inner_size(width as f64, height as f64)
                .build()?;

            // Counteract WebView2 DPI auto-scaling. The webview renders at
            // the system scale factor (e.g. 1.5x on 150% display). Apply a
            // partial correction so content is close to 1:1 but slightly
            // larger for readability.
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
        .run(tauri::generate_context!())
        .expect("error running tauri application");
}
