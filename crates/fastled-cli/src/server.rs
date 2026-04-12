//! Embedded HTTP/HTTPS static file server.
//!
//! Serves compiled FastLED output (JS, WASM, HTML) with the correct
//! COOP/COEP headers for SharedArrayBuffer support.  When index.html
//! does not exist yet (compilation in progress) a built-in loading page
//! is returned that polls `/build-status.json` for live updates.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::State;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use tower_http::set_header::SetResponseHeaderLayer;

// ---------------------------------------------------------------------------
// Loading page (shown while compilation is in progress)
// ---------------------------------------------------------------------------

const LOADING_PAGE: &str = r#"<!DOCTYPE html>
<html><head>
<meta charset="UTF-8">
<title>FastLED - Compiling...</title>
<style>
  body { background: #121212; color: #e0e0e0; font-family: monospace;
         display: flex; justify-content: center; align-items: center;
         height: 100vh; margin: 0; flex-direction: column; }
  .spinner { width: 40px; height: 40px; border: 4px solid #333;
             border-top: 4px solid #4fc3f7; border-radius: 50%;
             animation: spin 1s linear infinite; margin-bottom: 20px; }
  @keyframes spin { to { transform: rotate(360deg); } }
  #status { font-size: 1.2em; }
  #log { margin-top: 20px; max-width: 80%; max-height: 40vh;
         overflow-y: auto; font-size: 0.85em; color: #888;
         white-space: pre-wrap; text-align: left; }
</style>
<script>
  async function poll() {
    try {
      const r = await fetch('/build-status.json');
      if (r.ok) {
        const s = await r.json();
        document.getElementById('status').textContent = s.message || 'Compiling...';
        if (s.log) document.getElementById('log').textContent = s.log;
        if (s.status === 'success') { location.reload(); return; }
      }
    } catch(e) {}
    setTimeout(poll, 500);
  }
  poll();
</script>
</head><body>
<div class="spinner"></div>
<div id="status">Compiling...</div>
<div id="log"></div>
</body></html>"#;

// ---------------------------------------------------------------------------
// Server state
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct AppState {
    serve_dir: Arc<PathBuf>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Serve index.html or the loading page if it doesn't exist yet.
async fn serve_index(State(state): State<AppState>) -> Response {
    let index = state.serve_dir.join("index.html");
    if index.is_file() {
        match tokio::fs::read(&index).await {
            Ok(data) => (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                data,
            )
                .into_response(),
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    } else {
        Html(LOADING_PAGE).into_response()
    }
}

/// Serve any file from the output directory.
async fn serve_file(
    State(state): State<AppState>,
    axum::extract::Path(path): axum::extract::Path<String>,
) -> Response {
    let file_path = state.serve_dir.join(&path);

    // Prevent directory traversal.
    let canonical = match file_path.canonicalize() {
        Ok(p) => p,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };
    let serve_canonical = match state.serve_dir.canonicalize() {
        Ok(p) => p,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    if !canonical.starts_with(&serve_canonical) {
        return StatusCode::FORBIDDEN.into_response();
    }

    match tokio::fs::read(&file_path).await {
        Ok(data) => {
            let mime = mime_for_path(&path);
            (StatusCode::OK, [(header::CONTENT_TYPE, mime)], data).into_response()
        }
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

fn mime_for_path(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "mjs" => "text/javascript; charset=utf-8",
        "wasm" => "application/wasm",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "ttf" | "otf" => "font/ttf",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "map" => "application/json",
        _ => "application/octet-stream",
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Start the HTTP server in a background tokio task.
///
/// Returns the actual address the server bound to (useful when port 0 is
/// requested for automatic assignment).
pub async fn start_server(serve_dir: PathBuf, port: u16) -> anyhow::Result<SocketAddr> {
    let state = AppState {
        serve_dir: Arc::new(serve_dir),
    };

    let app = Router::new()
        .route("/", get(serve_index))
        .route("/{*path}", get(serve_file))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::HeaderName::from_static("cross-origin-embedder-policy"),
            HeaderValue::from_static("credentialless"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::HeaderName::from_static("cross-origin-opener-policy"),
            HeaderValue::from_static("same-origin"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-cache, no-store, must-revalidate"),
        ))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port))).await?;
    let addr = listener.local_addr()?;

    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });

    Ok(addr)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Helper: create a temp dir, start the server, return (addr, dir).
    async fn setup_server() -> (SocketAddr, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let addr = start_server(dir.path().to_path_buf(), 0).await.unwrap();
        // Give the server a moment to bind.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        (addr, dir)
    }

    #[tokio::test]
    async fn test_loading_page_when_no_index_html() {
        let (addr, _dir) = setup_server().await;
        let resp = reqwest::get(format!("http://{addr}/")).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.text().await.unwrap();
        assert!(
            body.contains("Compiling..."),
            "expected loading page, got: {body}"
        );
    }

    #[tokio::test]
    async fn test_serves_index_html_when_present() {
        let (addr, dir) = setup_server().await;
        fs::write(dir.path().join("index.html"), "<html>OK</html>").unwrap();
        let resp = reqwest::get(format!("http://{addr}/")).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.text().await.unwrap();
        assert!(
            body.contains("OK"),
            "expected index.html content, got: {body}"
        );
    }

    #[tokio::test]
    async fn test_serves_js_with_correct_mime() {
        let (addr, dir) = setup_server().await;
        fs::write(dir.path().join("app.js"), "console.log('hi')").unwrap();
        let resp = reqwest::get(format!("http://{addr}/app.js")).await.unwrap();
        assert_eq!(resp.status(), 200);
        let ct = resp
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(ct.contains("javascript"), "expected JS mime, got: {ct}");
    }

    #[tokio::test]
    async fn test_serves_wasm_with_correct_mime() {
        let (addr, dir) = setup_server().await;
        fs::write(dir.path().join("fastled.wasm"), [0x00, 0x61, 0x73, 0x6d]).unwrap();
        let resp = reqwest::get(format!("http://{addr}/fastled.wasm"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let ct = resp
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(ct.contains("wasm"), "expected WASM mime, got: {ct}");
    }

    #[tokio::test]
    async fn test_coop_coep_headers() {
        let (addr, _dir) = setup_server().await;
        let resp = reqwest::get(format!("http://{addr}/")).await.unwrap();
        let coep = resp
            .headers()
            .get("cross-origin-embedder-policy")
            .unwrap()
            .to_str()
            .unwrap();
        let coop = resp
            .headers()
            .get("cross-origin-opener-policy")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(coep, "credentialless");
        assert_eq!(coop, "same-origin");
    }

    #[tokio::test]
    async fn test_404_for_missing_file() {
        let (addr, _dir) = setup_server().await;
        let resp = reqwest::get(format!("http://{addr}/nonexistent.js"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 404);
    }

    #[tokio::test]
    async fn test_build_status_json_served() {
        let (addr, dir) = setup_server().await;
        // Initially no build-status.json -> 404
        let resp = reqwest::get(format!("http://{addr}/build-status.json"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 404);

        // Write status file -> 200
        fs::write(
            dir.path().join("build-status.json"),
            r#"{"status":"compiling","message":"Building..."}"#,
        )
        .unwrap();
        let resp = reqwest::get(format!("http://{addr}/build-status.json"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.text().await.unwrap();
        assert!(body.contains("compiling"));
    }

    #[tokio::test]
    async fn test_directory_traversal_blocked() {
        let (addr, dir) = setup_server().await;
        // Create a file outside the serve dir
        let parent = dir.path().parent().unwrap();
        fs::write(parent.join("secret.txt"), "top secret").unwrap();
        let resp = reqwest::get(format!("http://{addr}/../secret.txt"))
            .await
            .unwrap();
        // Should not serve files outside the serve dir
        assert_ne!(resp.status(), 200);
    }
}
