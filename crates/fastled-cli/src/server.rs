//! Embedded HTTP/HTTPS static file server.
//!
//! Serves compiled FastLED output (JS, WASM, HTML) with the correct
//! COOP/COEP headers for SharedArrayBuffer support.  When index.html
//! does not exist yet (compilation in progress) a built-in loading page
//! is returned that polls `/build-status.json` for live updates.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::State;
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tower_http::cors::{Any, CorsLayer};
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
  #log { margin-top: 20px; width: 80%; max-height: 50vh;
         overflow-y: auto; font-size: 0.85em; color: #aaa;
         white-space: pre-wrap; text-align: left; line-height: 1.4;
         padding: 8px; background: #1a1a1a; border-radius: 4px; }
</style>
<script>
  const logEl = document.addEventListener('DOMContentLoaded', () => {
    const log = document.getElementById('log');
    const status = document.getElementById('status');

    function appendLog(line) {
      log.textContent += line + '\n';
      log.scrollTop = log.scrollHeight;
    }

    // Try SSE first, fall back to polling.
    if (typeof EventSource !== 'undefined') {
      const es = new EventSource('/build-stream');
      es.onmessage = (e) => {
        try {
          const d = JSON.parse(e.data);
          if (d.type === 'log') {
            appendLog(d.line);
          } else if (d.type === 'status') {
            status.textContent = d.message || d.status;
            if (d.status === 'success') { es.close(); location.reload(); }
          }
        } catch(err) {}
      };
      es.onerror = () => { es.close(); poll(); };
    } else {
      poll();
    }
  });

  async function poll() {
    try {
      const r = await fetch('/build-status.json');
      if (r.ok) {
        const s = await r.json();
        document.getElementById('status').textContent = s.message || 'Compiling...';
        if (s.status === 'success') { location.reload(); return; }
      }
    } catch(e) {}
    setTimeout(poll, 500);
  }
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
    /// Broadcast channel for SSE build streaming.  `None` when serving a
    /// static directory (no compilation happening).
    build_tx: Option<broadcast::Sender<String>>,
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

/// SSE endpoint that streams build log lines and status events.
async fn build_stream(State(state): State<AppState>) -> Response {
    let tx = match &state.build_tx {
        Some(tx) => tx,
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    let rx = tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| {
        result
            .ok()
            .map(|data| Ok::<_, Infallible>(Event::default().data(data)))
    });

    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
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
pub async fn start_server(
    serve_dir: PathBuf,
    port: u16,
    build_tx: Option<broadcast::Sender<String>>,
) -> anyhow::Result<SocketAddr> {
    let state = AppState {
        serve_dir: Arc::new(serve_dir),
        build_tx,
    };

    let app = Router::new()
        .route("/", get(serve_index))
        .route("/build-stream", get(build_stream))
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
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([
                    Method::GET,
                    Method::POST,
                    Method::PUT,
                    Method::DELETE,
                    Method::OPTIONS,
                ])
                .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION]),
        )
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
        let addr = start_server(dir.path().to_path_buf(), 0, None)
            .await
            .unwrap();
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

    // ------------------------------------------------------------------
    // SSE build-stream tests
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn test_sse_returns_404_without_broadcast() {
        // Server started without broadcast channel → /build-stream returns 404.
        let (addr, _dir) = setup_server().await;
        let resp = reqwest::get(format!("http://{addr}/build-stream"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 404);
    }

    #[tokio::test]
    async fn test_sse_endpoint_streams_events() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, _rx) = broadcast::channel::<String>(16);
        let addr = start_server(dir.path().to_path_buf(), 0, Some(tx.clone()))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let url = format!("http://{addr}/build-stream");

        // Connect to SSE endpoint.
        let client = reqwest::Client::new();
        let mut resp = client.get(&url).send().await.unwrap();
        assert_eq!(resp.status(), 200);
        let ct = resp
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert!(
            ct.contains("text/event-stream"),
            "expected event-stream content-type, got: {ct}"
        );

        // Give the server handler a moment to subscribe to the broadcast.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Send test events.
        tx.send(r#"{"type":"log","line":"Building sketch...","stream":"stdout"}"#.to_string())
            .unwrap();
        tx.send(r#"{"type":"status","status":"success","message":"Done"}"#.to_string())
            .unwrap();

        // Read SSE chunks until we see both events (or timeout).
        let mut collected = String::new();
        let deadline = std::time::Duration::from_secs(3);
        while let Ok(Ok(Some(chunk))) = tokio::time::timeout(deadline, resp.chunk()).await {
            collected.push_str(&String::from_utf8_lossy(&chunk));
            if collected.contains("Building sketch...") && collected.contains("success") {
                break;
            }
        }

        assert!(
            collected.contains("Building sketch..."),
            "expected log line in SSE body, got: {collected}"
        );
        assert!(
            collected.contains("success"),
            "expected status event in SSE body, got: {collected}"
        );
    }

    #[tokio::test]
    async fn test_loading_page_contains_eventsource() {
        let (addr, _dir) = setup_server().await;
        let resp = reqwest::get(format!("http://{addr}/")).await.unwrap();
        let body = resp.text().await.unwrap();
        assert!(
            body.contains("EventSource"),
            "loading page should use EventSource for SSE"
        );
        assert!(
            body.contains("/build-stream"),
            "loading page should connect to /build-stream"
        );
    }
}
