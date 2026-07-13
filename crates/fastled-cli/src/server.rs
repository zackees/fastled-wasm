//! Embedded HTTP/HTTPS static file server.
//!
//! Serves compiled FastLED output (JS, WASM, HTML) with the correct
//! COOP/COEP headers for SharedArrayBuffer support.  When index.html
//! does not exist yet (compilation in progress) a built-in loading page
//! is returned that polls `/build-status.json` for live updates.

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tower_http::cors::{Any, CorsLayer};
use tower_http::set_header::SetResponseHeaderLayer;

use crate::debug_symbols::{DebugSymbolResolver, ResolveError};

/// Shared, mutable handle to a DWARF source resolver.
///
/// The resolver is unknown when the server starts (we haven't run a build
/// yet) and is filled in after the first successful build. Wrapping it in
/// `Arc<RwLock<Option<_>>>` lets the watch loop swap it in without bouncing
/// the server.
pub type DebugSymbolHandle = Arc<RwLock<Option<DebugSymbolResolver>>>;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TestRuntimeConfig {
    pub(crate) wait_ms: f64,
    pub(crate) interval_ms: Option<f64>,
    pub(crate) screenshot_names: Vec<String>,
}

#[derive(Clone)]
pub(crate) struct TestServerOptions {
    pub(crate) runtime: TestRuntimeConfig,
    pub(crate) screenshot_paths: HashMap<String, PathBuf>,
    pub(crate) events: tokio::sync::mpsc::UnboundedSender<TestEvent>,
    pub(crate) token: String,
    pub(crate) sleep_permits: Arc<tokio::sync::Semaphore>,
}

#[derive(Debug)]
pub(crate) enum TestEvent {
    Ready,
    ViewerLog(String),
    ScreenshotSaved { name: String, path: PathBuf },
    Failure(String),
    Done(u8),
}

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
  .spinner.error { border-top-color: #ef5350; animation: none; }
  @keyframes spin { to { transform: rotate(360deg); } }
  #header { font-size: 1.2em; display: flex; align-items: baseline; gap: 12px; }
  #status { }
  #elapsed { color: #888; font-size: 0.9em; }
  #header.error #status { color: #ef5350; }
  #log { margin-top: 20px; width: 80%; max-height: 50vh;
         overflow-y: auto; font-size: 0.85em; color: #aaa;
         white-space: pre-wrap; text-align: left; line-height: 1.4;
         padding: 8px; background: #1a1a1a; border-radius: 4px; }
  #log div { min-height: 1.2em; }
  #log .warn { color: #ffd54f; }
  #log .err { color: #ef5350; }
</style>
<script>
  // Instant-launch flow (#148):
  //   * subscribe to /build-stream SSE, fall back to polling /build-status.json
  //   * on 'compiling' (including re-entry from a prior build) clear the log and
  //     reset the elapsed-time counter so a rebuild shows a fresh view
  //   * on 'success' reload to the real index.html (now on disk)
  //   * on 'error' stay on this page; show error styling; keep the log visible
  //     so the user can read what failed and wait for the next rebuild
  document.addEventListener('DOMContentLoaded', () => {
    const log = document.getElementById('log');
    const status = document.getElementById('status');
    const elapsed = document.getElementById('elapsed');
    const spinner = document.getElementById('spinner');
    const header = document.getElementById('header');
    let buildStart = Date.now();
    let elapsedTimer = null;
    // Auto-scroll follows the log tail unless the user scrolled up to read.
    let userScrolled = false;
    log.addEventListener('scroll', () => {
      userScrolled = log.scrollTop + log.clientHeight < log.scrollHeight - 10;
    });

    function startTimer() {
      stopTimer();
      buildStart = Date.now();
      elapsedTimer = setInterval(() => {
        elapsed.textContent = ((Date.now() - buildStart) / 1000).toFixed(1) + 's';
      }, 100);
    }
    function stopTimer() {
      if (elapsedTimer !== null) { clearInterval(elapsedTimer); elapsedTimer = null; }
    }
    function setBuilding(message) {
      header.classList.remove('error');
      spinner.classList.remove('error');
      status.textContent = message || 'Compiling...';
      log.textContent = '';
      userScrolled = false;
      startTimer();
    }
    function setError(message) {
      header.classList.add('error');
      spinner.classList.add('error');
      status.textContent = message || 'Compilation failed';
      stopTimer();
      // Jump to the first diagnostic: emcc errors are usually followed by
      // long noise, so the bottom of the log is the wrong place to land.
      const firstErr = log.querySelector('.err');
      if (firstErr) {
        log.scrollTop = Math.max(0, firstErr.offsetTop - log.offsetTop - 8);
      }
    }
    function classifyLine(line, stream) {
      const l = line.toLowerCase();
      if (l.includes('error:') || l.includes('fatal error') ||
          l.includes('undefined symbol') ||
          (stream === 'stderr' && l.includes('failed'))) { return 'err'; }
      if (l.includes('warning:')) { return 'warn'; }
      return '';
    }
    function appendLog(line, stream) {
      const div = document.createElement('div');
      div.textContent = line;
      const cls = classifyLine(line, stream || 'stdout');
      if (cls) { div.className = cls; }
      log.appendChild(div);
      if (!userScrolled) { log.scrollTop = log.scrollHeight; }
    }

    startTimer();
    let lastStatus = 'compiling';

    function handleStatus(s, message) {
      if (s === 'compiling' && lastStatus !== 'compiling') {
        setBuilding(message);
      } else if (s === 'success') {
        stopTimer();
        location.reload();
      } else if (s === 'error') {
        setError(message);
      } else if (s === 'compiling') {
        // First compiling event — keep the in-progress styling, just update text.
        status.textContent = message || 'Compiling...';
      }
      lastStatus = s;
    }

    if (typeof EventSource !== 'undefined') {
      const es = new EventSource('/build-stream');
      es.onmessage = (e) => {
        try {
          const d = JSON.parse(e.data);
          if (d.type === 'log') {
            appendLog(d.line, d.stream);
          } else if (d.type === 'status') {
            handleStatus(d.status, d.message);
            if (d.status === 'success') { es.close(); }
          }
        } catch(err) {}
      };
      es.onerror = () => { es.close(); poll(); };
    } else {
      poll();
    }

    window.__pollBuildStatus = poll;
    async function poll() {
      try {
        const r = await fetch('/build-status.json');
        if (r.ok) {
          const s = await r.json();
          handleStatus(s.status, s.message);
          if (s.status === 'success') { return; }
        }
      } catch(e) {}
      setTimeout(poll, 500);
    }
  });
</script>
</head><body>
<div class="spinner" id="spinner"></div>
<div id="header"><span id="status">Compiling...</span><span id="elapsed">0.0s</span></div>
<div id="log"></div>
</body></html>"#;

const TEST_WORKER_WEBGL_PREFIX: &str = r#"
const __fastledOriginalOffscreenGetContext = OffscreenCanvas.prototype.getContext;
let __fastledTestWebglReported = false;
const __fastledTestContexts = [];

const __fastledTestCaptureFrame = async (request) => {
  const id = request.id;
  try {
    if (__fastledTestContexts.length === 0) throw new Error('FastLED WebGL context is unavailable');
    let selected = null;
    const manager = typeof workerState !== 'undefined' && workerState.graphicsManager;
    if (manager) {
      const freshFrameData = extractFrameData();
      let nonzeroFrameValues = 0;
      if (freshFrameData) {
        for (const strip of freshFrameData) {
          for (const value of strip.pixel_data || []) if (value > 0) nonzeroFrameValues += 1;
        }
        if (Object.keys(manager.screenMaps || {}).length === 0 && workerState.wasmFunctions) {
          const Module = workerState.fastledModule;
          const screenMapSizePtr = Module._malloc(4);
          const screenMapDataPtr = workerState.wasmFunctions.getScreenMapData(screenMapSizePtr);
          if (screenMapDataPtr) {
            const screenMapSize = Module.getValue(screenMapSizePtr, 'i32');
            const screenMaps = JSON.parse(Module.UTF8ToString(screenMapDataPtr, screenMapSize));
            manager.updateScreenMap(screenMaps);
            workerState.screenMaps = screenMaps;
            workerState.wasmFunctions.freeFrameData(screenMapDataPtr);
          }
          Module._free(screenMapSizePtr);
        }
        manager.updateCanvas(freshFrameData);
      }
      postMessage({
        type: 'stdout',
        payload: { text: `[fastled-test] synchronous frame values=${nonzeroFrameValues} screenMaps=${Object.keys(manager.screenMaps || {}).length}` }
      });
    }
    for (const entry of __fastledTestContexts) {
      const width = entry.canvas.width;
      const height = entry.canvas.height;
      const pixels = new Uint8Array(width * height * 4);
      const previousFramebuffer = entry.gl.getParameter(entry.gl.FRAMEBUFFER_BINDING);
      let readError;
      try {
        entry.gl.bindFramebuffer(entry.gl.FRAMEBUFFER, null);
        entry.gl.finish();
        entry.gl.readPixels(0, 0, width, height, entry.gl.RGBA, entry.gl.UNSIGNED_BYTE, pixels);
        readError = entry.gl.getError();
      } finally {
        entry.gl.bindFramebuffer(entry.gl.FRAMEBUFFER, previousFramebuffer);
      }
      if (readError !== entry.gl.NO_ERROR) {
        throw new Error(`WebGL readPixels failed with error 0x${readError.toString(16)}`);
      }
      let nonBlackPixels = 0;
      let varied = false;
      const first = pixels.slice(0, 4);
      for (let offset = 0; offset < pixels.length; offset += 4) {
        if (pixels[offset] || pixels[offset + 1] || pixels[offset + 2]) nonBlackPixels += 1;
        if (pixels[offset] !== first[0] || pixels[offset + 1] !== first[1]
            || pixels[offset + 2] !== first[2] || pixels[offset + 3] !== first[3]) varied = true;
      }
      postMessage({
        type: 'stdout',
        payload: { text: `[fastled-test] context ${entry.kind} ${width}x${height} nonBlack=${nonBlackPixels} varied=${varied}` }
      });
      const candidate = { canvas: entry.canvas, gl: entry.gl, width, height, pixels, nonBlackPixels, varied };
      if (!selected || candidate.nonBlackPixels > selected.nonBlackPixels
          || (candidate.nonBlackPixels === selected.nonBlackPixels && candidate.varied && !selected.varied)
          || (candidate.nonBlackPixels === selected.nonBlackPixels && candidate.varied === selected.varied
              && candidate.width * candidate.height > selected.width * selected.height)) {
        selected = candidate;
      }
    }
    const { width, height, pixels, nonBlackPixels, varied } = selected;
    const flipped = new Uint8ClampedArray(pixels.length);
    const rowBytes = width * 4;
    for (let y = 0; y < height; y += 1) {
      const source = (height - y - 1) * rowBytes;
      flipped.set(pixels.subarray(source, source + rowBytes), y * rowBytes);
    }
    const output = new OffscreenCanvas(width, height);
    const context = output.getContext('2d');
    if (!context) throw new Error('2D screenshot context is unavailable');
    context.putImageData(new ImageData(flipped, width, height), 0, 0);
    const blob = await output.convertToBlob({ type: 'image/png' });
    const bytes = await blob.arrayBuffer();
    postMessage({
      type: 'fastled_test_capture_response',
      id,
      bytes,
      stats: { width, height, nonBlackPixels, varied }
    }, [bytes]);
  } catch (error) {
    postMessage({
      type: 'fastled_test_capture_response',
      id,
      error: error && error.stack ? error.stack : String(error)
    });
  }
};

OffscreenCanvas.prototype.getContext = function(kind, attributes) {
  if (kind === 'webgl' || kind === 'webgl2' || kind === 'experimental-webgl') {
    attributes = Object.assign({}, attributes || {}, { preserveDrawingBuffer: true });
    if (!__fastledTestWebglReported) {
      __fastledTestWebglReported = true;
      postMessage({
        type: 'stdout',
        payload: { text: '[fastled-test] OffscreenCanvas preserveDrawingBuffer enabled' }
      });
    }
  }
  const context = __fastledOriginalOffscreenGetContext.call(this, kind, attributes);
  if (context && (kind === 'webgl' || kind === 'webgl2' || kind === 'experimental-webgl')) {
    __fastledTestContexts.push({ gl: context, canvas: this, kind });
    const effectiveAttributes = context.getContextAttributes();
    postMessage({
      type: 'stdout',
      payload: { text: `[fastled-test] WebGL context ${kind} canvas=${this.width}x${this.height} preserveDrawingBuffer=${effectiveAttributes && effectiveAttributes.preserveDrawingBuffer}` }
    });
  }
  return context;
};
self.addEventListener('message', (event) => {
  if (!event.data || event.data.type !== 'fastled_test_capture') return;
  event.stopImmediatePropagation();
  void __fastledTestCaptureFrame({ id: event.data.id });
  postMessage({
    type: 'stdout',
    payload: { text: '[fastled-test] capture requested' }
  });
});
"#;

// ---------------------------------------------------------------------------
// Server state
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct AppState {
    serve_dir: Arc<PathBuf>,
    /// Broadcast channel for SSE build streaming.  `None` when serving a
    /// static directory (no compilation happening).
    build_tx: Option<broadcast::Sender<String>>,
    /// Shared resolver for DWARF source paths. Empty until a successful
    /// build populates it.
    debug_symbols: DebugSymbolHandle,
    test: Option<Arc<TestServerOptions>>,
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
        Err(_) => {
            return serve_debug_source_url(&state, &path)
                .await
                .unwrap_or_else(|| StatusCode::NOT_FOUND.into_response());
        }
    };
    let serve_canonical = match state.serve_dir.canonicalize() {
        Ok(p) => p,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    if !canonical.starts_with(&serve_canonical) {
        return StatusCode::FORBIDDEN.into_response();
    }

    match tokio::fs::read(&file_path).await {
        Ok(mut data) => {
            if state.test.is_some() && path == "fastled_background_worker.js" {
                let mut patched = TEST_WORKER_WEBGL_PREFIX.as_bytes().to_vec();
                patched.extend_from_slice(&data);
                data = patched;
            }
            let mime = mime_for_path(&path);
            (StatusCode::OK, [(header::CONTENT_TYPE, mime)], data).into_response()
        }
        Err(_) => serve_debug_source_url(&state, &path)
            .await
            .unwrap_or_else(|| StatusCode::NOT_FOUND.into_response()),
    }
}

async fn serve_debug_source_url(state: &AppState, request_path: &str) -> Option<Response> {
    let resolver = state.debug_symbols.read().ok()?.clone()?;
    match resolver.resolve(request_path, true) {
        Ok(file) => match tokio::fs::read(&file).await {
            Ok(bytes) => Some(
                (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                    bytes,
                )
                    .into_response(),
            ),
            Err(_) => Some(StatusCode::INTERNAL_SERVER_ERROR.into_response()),
        },
        Err(ResolveError::NotFound(_)) => Some(StatusCode::NOT_FOUND.into_response()),
        Err(ResolveError::Invalid(_)) => None,
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

/// `POST /viewer-log` — receive console/error lines forwarded from the Tauri
/// viewer's injected logging script (enabled via `FASTLED_VIEWER_LOGS`) and
/// echo them to stderr so viewer-side failures are diagnosable.
fn test_request_authorized(test: &TestServerOptions, headers: &HeaderMap) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|value| value == test.token)
}

async fn viewer_log(State(state): State<AppState>, headers: HeaderMap, body: String) -> Response {
    if let Some(test) = &state.test {
        if !test_request_authorized(test, &headers) {
            return StatusCode::UNAUTHORIZED.into_response();
        }
    }
    for line in body.lines() {
        eprintln!("[viewer] {line}");
        if let Some(test) = &state.test {
            let _ = test.events.send(TestEvent::ViewerLog(line.to_string()));
        }
    }
    StatusCode::NO_CONTENT.into_response()
}

async fn test_config(State(state): State<AppState>, headers: HeaderMap) -> Response {
    match &state.test {
        Some(test) if test_request_authorized(test, &headers) => {
            Json(test.runtime.clone()).into_response()
        }
        Some(_) => StatusCode::UNAUTHORIZED.into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn test_ready(State(state): State<AppState>, headers: HeaderMap) -> Response {
    match &state.test {
        Some(test) if test_request_authorized(test, &headers) => {
            let _ = test.events.send(TestEvent::Ready);
            StatusCode::NO_CONTENT.into_response()
        }
        Some(_) => StatusCode::UNAUTHORIZED.into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[derive(Deserialize)]
struct TestSleepQuery {
    ms: f64,
}

async fn test_sleep(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<TestSleepQuery>,
) -> Response {
    let Some(test) = &state.test else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !test_request_authorized(test, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Ok(duration) = std::time::Duration::try_from_secs_f64(query.ms / 1_000.0) else {
        return (StatusCode::BAD_REQUEST, "invalid sleep duration").into_response();
    };
    let max_ms = test
        .runtime
        .interval_ms
        .unwrap_or(0.0)
        .max(test.runtime.wait_ms);
    if query.ms > max_ms {
        return (StatusCode::BAD_REQUEST, "sleep exceeds test schedule").into_response();
    }
    let Ok(_permit) = test.sleep_permits.clone().try_acquire_owned() else {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    };
    tokio::time::sleep(duration).await;
    StatusCode::NO_CONTENT.into_response()
}

#[derive(Deserialize)]
struct ScreenshotQuery {
    name: String,
}

async fn viewer_screenshot(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ScreenshotQuery>,
    body: Bytes,
) -> Response {
    let Some(test) = &state.test else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !test_request_authorized(test, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Some(path) = test.screenshot_paths.get(&query.name) else {
        return (StatusCode::BAD_REQUEST, "unknown screenshot name").into_response();
    };
    if body.len() < 8 || body[..8] != [137, 80, 78, 71, 13, 10, 26, 10] {
        let message = format!("viewer returned invalid PNG data for {}", path.display());
        let _ = test.events.send(TestEvent::Failure(message.clone()));
        return (StatusCode::BAD_REQUEST, message).into_response();
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        if let Err(error) = tokio::fs::create_dir_all(parent).await {
            let message = format!("could not create screenshot directory: {error}");
            let _ = test.events.send(TestEvent::Failure(message.clone()));
            return (StatusCode::INTERNAL_SERVER_ERROR, message).into_response();
        }
    }
    if let Err(error) = tokio::fs::write(path, &body).await {
        let message = format!("could not write screenshot {}: {error}", path.display());
        let _ = test.events.send(TestEvent::Failure(message.clone()));
        return (StatusCode::INTERNAL_SERVER_ERROR, message).into_response();
    }
    let _ = test.events.send(TestEvent::ScreenshotSaved {
        name: query.name,
        path: path.clone(),
    });
    StatusCode::NO_CONTENT.into_response()
}

async fn test_done(State(state): State<AppState>, headers: HeaderMap, body: String) -> Response {
    let Some(test) = &state.test else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !test_request_authorized(test, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let code = body.trim().parse::<u8>().unwrap_or(1);
    let _ = test.events.send(TestEvent::Done(code));
    StatusCode::NO_CONTENT.into_response()
}

// ---------------------------------------------------------------------------
// DWARF debug source handlers
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct DwarfSourceRequest {
    path: String,
}

/// `POST /dwarfsource` — given `{"path": "<dwarf-path>"}`, return the source
/// text for the file the path maps to. Empty/missing payload, or a path that
/// escapes the configured roots, returns 400. Unknown files return 404.
async fn dwarf_source(
    State(state): State<AppState>,
    Json(payload): Json<DwarfSourceRequest>,
) -> Response {
    let resolver = match state.debug_symbols.read() {
        Ok(guard) => guard.clone(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR).into_response(),
    };

    let Some(resolver) = resolver else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "debug source resolver unavailable"})),
        )
            .into_response();
    };

    let request_path = payload.path.trim();
    if request_path.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "missing path"})),
        )
            .into_response();
    }

    match resolver.resolve(request_path, true) {
        Ok(file) => match tokio::fs::read(&file).await {
            Ok(bytes) => (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                bytes,
            )
                .into_response(),
            Err(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": err.to_string()})),
            )
                .into_response(),
        },
        Err(ResolveError::NotFound(msg)) => {
            (StatusCode::NOT_FOUND, Json(json!({ "error": msg }))).into_response()
        }
        Err(ResolveError::Invalid(msg)) => {
            (StatusCode::BAD_REQUEST, Json(json!({ "error": msg }))).into_response()
        }
    }
}

/// `GET /debug/source-roots` — return the named source roots the resolver
/// knows about. Useful for tooling that wants to verify configuration.
async fn debug_source_roots(State(state): State<AppState>) -> Response {
    let resolver = match state.debug_symbols.read() {
        Ok(guard) => guard.clone(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR).into_response(),
    };

    let roots = match resolver {
        Some(resolver) => resolver
            .config()
            .source_roots()
            .into_iter()
            .map(|(prefix, path)| json!({"prefix": prefix, "path": path.display().to_string()}))
            .collect::<Vec<_>>(),
        None => Vec::new(),
    };
    Json(json!({ "roots": roots })).into_response()
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
/// requested for automatic assignment). `debug_symbols` is shared with the
/// caller so a build can populate the resolver after the server starts.
pub async fn start_server(
    serve_dir: PathBuf,
    port: u16,
    build_tx: Option<broadcast::Sender<String>>,
    debug_symbols: DebugSymbolHandle,
    test: Option<TestServerOptions>,
) -> anyhow::Result<SocketAddr> {
    let state = AppState {
        serve_dir: Arc::new(serve_dir),
        build_tx,
        debug_symbols,
        test: test.map(Arc::new),
    };

    let app = Router::new()
        .route("/", get(serve_index))
        .route("/build-stream", get(build_stream))
        .route("/dwarfsource", post(dwarf_source))
        .route("/viewer-log", post(viewer_log))
        .route("/test-config", get(test_config))
        .route("/test-ready", post(test_ready))
        .route("/test-sleep", post(test_sleep))
        .route("/viewer-screenshot", post(viewer_screenshot))
        .route("/test-done", post(test_done))
        .route("/debug/source-roots", get(debug_source_roots))
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
        .layer(DefaultBodyLimit::max(64 * 1024 * 1024))
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

    fn empty_handle() -> DebugSymbolHandle {
        Arc::new(RwLock::new(None))
    }

    /// Helper: create a temp dir, start the server, return (addr, dir).
    async fn setup_server() -> (SocketAddr, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let addr = start_server(dir.path().to_path_buf(), 0, None, empty_handle(), None)
            .await
            .unwrap();
        // Give the server a moment to bind.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        (addr, dir)
    }

    #[tokio::test]
    async fn test_viewer_log_endpoint_accepts_posts() {
        let (addr, _dir) = setup_server().await;
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("http://{addr}/viewer-log"))
            .body("error: something broke")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 204);
    }

    #[tokio::test]
    async fn test_runtime_endpoints_use_preconfigured_screenshot_paths() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("fastled_background_worker.js"),
            "console.log('worker');",
        )
        .unwrap();
        let screenshot = dir.path().join("artifacts").join("frame.png");
        let (events, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let options = TestServerOptions {
            runtime: TestRuntimeConfig {
                wait_ms: 25.0,
                interval_ms: Some(10.0),
                screenshot_names: vec!["frame-0".to_string()],
            },
            screenshot_paths: HashMap::from([("frame-0".to_string(), screenshot.clone())]),
            events,
            token: "test-token".to_string(),
            sleep_permits: Arc::new(tokio::sync::Semaphore::new(4)),
        };
        let addr = start_server(
            dir.path().to_path_buf(),
            0,
            None,
            empty_handle(),
            Some(options),
        )
        .await
        .unwrap();
        let client = reqwest::Client::new();

        let unauthorized = client
            .get(format!("http://{addr}/test-config"))
            .send()
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
        let config_response = client
            .get(format!("http://{addr}/test-config"))
            .bearer_auth("test-token")
            .send()
            .await
            .unwrap();
        let config: serde_json::Value =
            serde_json::from_str(&config_response.text().await.unwrap()).unwrap();
        assert_eq!(config["waitMs"].as_f64(), Some(25.0));
        assert_eq!(config["screenshotNames"][0], "frame-0");

        let response = client
            .post(format!("http://{addr}/test-sleep?ms=1"))
            .bearer_auth("test-token")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        let response = client
            .post(format!("http://{addr}/test-sleep?ms=-1"))
            .bearer_auth("test-token")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let response = client
            .post(format!("http://{addr}/test-sleep?ms=26"))
            .bearer_auth("test-token")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let worker = client
            .get(format!("http://{addr}/fastled_background_worker.js"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(worker.starts_with("\nconst __fastledOriginalOffscreenGetContext"));
        assert!(worker.contains("preserveDrawingBuffer: true"));
        assert!(worker.contains("console.log('worker');"));
        assert!(worker.contains("gl.readPixels"));
        assert!(worker.contains("fastled_test_capture_response"));

        let response = client
            .post(format!("http://{addr}/viewer-screenshot?name=..%2Fescape"))
            .bearer_auth("test-token")
            .body(vec![137, 80, 78, 71, 13, 10, 26, 10])
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(!dir.path().join("escape").exists());

        let png = vec![137, 80, 78, 71, 13, 10, 26, 10, 1, 2, 3];
        let response = client
            .post(format!("http://{addr}/viewer-screenshot?name=frame-0"))
            .bearer_auth("test-token")
            .body(png.clone())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(fs::read(&screenshot).unwrap(), png);
        assert!(matches!(
            rx.recv().await,
            Some(TestEvent::ScreenshotSaved { name, .. }) if name == "frame-0"
        ));
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

    /// Instant-launch failure UX (#148 acceptance criterion 5):
    /// when a compile has failed and `index.html` is absent, `/` must still
    /// return the in-browser loading page (which surfaces the error log) —
    /// it must NOT 404 or navigate to a stale page.
    #[tokio::test]
    async fn test_loading_page_stays_when_build_failed_and_no_index_html() {
        let (addr, dir) = setup_server().await;
        fs::write(
            dir.path().join("build-status.json"),
            r#"{"status":"error","message":"Compilation failed"}"#,
        )
        .unwrap();
        let resp = reqwest::get(format!("http://{addr}/")).await.unwrap();
        assert_eq!(
            resp.status(),
            200,
            "viewer should land on the loading page, not 404/redirect, when compile failed"
        );
        let body = resp.text().await.unwrap();
        assert!(
            body.contains("Compiling..."),
            "expected loading page, got: {body}"
        );
        // Sanity check: the embedded JS knows how to render the error state.
        assert!(
            body.contains("setError"),
            "loading page must include error-handling branch"
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
        let addr = start_server(
            dir.path().to_path_buf(),
            0,
            Some(tx.clone()),
            empty_handle(),
            None,
        )
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

    /// Live compile log UX (#153): the loading page must classify and color
    /// warning/error lines, follow the log tail, and never show a fake
    /// progress bar (build step counts are unknowable).
    #[test]
    fn loading_page_colors_log_lines_and_has_no_progress_bar() {
        assert!(
            LOADING_PAGE.contains("classifyLine"),
            "loading page must classify log lines"
        );
        assert!(
            LOADING_PAGE.contains("warning:") && LOADING_PAGE.contains(".warn"),
            "warnings must get the yellow .warn style"
        );
        assert!(
            LOADING_PAGE.contains("error:") && LOADING_PAGE.contains(".err"),
            "errors must get the red .err style"
        );
        assert!(
            LOADING_PAGE.contains("spinner"),
            "indeterminate spinner is the only activity indicator"
        );
        assert!(
            !LOADING_PAGE.contains("<progress"),
            "no progress bar: build step counts are unknowable"
        );
        assert!(
            LOADING_PAGE.contains("userScrolled"),
            "auto-scroll must pause when the user scrolls up"
        );
    }

    // ------------------------------------------------------------------
    // DWARF source endpoint tests
    // ------------------------------------------------------------------

    fn json_body(value: serde_json::Value) -> String {
        value.to_string()
    }

    async fn post_json(addr: SocketAddr, path: &str, body: serde_json::Value) -> reqwest::Response {
        reqwest::Client::new()
            .post(format!("http://{addr}{path}"))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(json_body(body))
            .send()
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn dwarfsource_without_resolver_returns_400() {
        let (addr, _dir) = setup_server().await;
        let resp = post_json(
            addr,
            "/dwarfsource",
            serde_json::json!({"path": "sketchsource/foo.ino"}),
        )
        .await;
        assert_eq!(resp.status(), 400);
    }

    #[tokio::test]
    async fn debug_source_roots_empty_without_resolver() {
        let (addr, _dir) = setup_server().await;
        let resp = reqwest::get(format!("http://{addr}/debug/source-roots"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body: serde_json::Value = serde_json::from_str(&resp.text().await.unwrap()).unwrap();
        assert!(body["roots"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn dwarfsource_returns_resolved_file() {
        use crate::debug_symbols::{load_debug_symbol_config, DebugSymbolResolver};

        let dir = tempfile::tempdir().unwrap();
        let sketch_dir = dir.path().join("sketch");
        fs::create_dir_all(sketch_dir.join("src")).unwrap();
        let sketch_file = sketch_dir.join("src").join("demo.ino");
        fs::write(&sketch_file, "void setup() {}").unwrap();

        let resolver = DebugSymbolResolver::new(load_debug_symbol_config(sketch_dir, None, None));
        let handle: DebugSymbolHandle = Arc::new(RwLock::new(Some(resolver)));

        let addr = start_server(dir.path().to_path_buf(), 0, None, handle.clone(), None)
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let resp = post_json(
            addr,
            "/dwarfsource",
            serde_json::json!({"path": "sketchsource/src/demo.ino"}),
        )
        .await;
        assert_eq!(resp.status(), 200);
        let body = resp.text().await.unwrap();
        assert!(body.contains("void setup()"));

        let resp = reqwest::get(format!("http://{addr}/debug/source-roots"))
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_str(&resp.text().await.unwrap()).unwrap();
        let roots = body["roots"].as_array().unwrap();
        assert!(!roots.is_empty());
        assert!(roots
            .iter()
            .any(|r| r["prefix"].as_str() == Some("sketchsource")));
    }

    #[tokio::test]
    async fn source_map_style_get_returns_resolved_file() {
        use crate::debug_symbols::{load_debug_symbol_config, DebugSymbolResolver};

        let dir = tempfile::tempdir().unwrap();
        let serve_dir = dir.path().join("fastled_js");
        let sketch_dir = dir.path().join("sketch");
        fs::create_dir_all(sketch_dir.join("src")).unwrap();
        fs::create_dir_all(&serve_dir).unwrap();
        fs::write(sketch_dir.join("src").join("demo.ino"), "void loop() {}").unwrap();

        let resolver = DebugSymbolResolver::new(load_debug_symbol_config(sketch_dir, None, None));
        let handle: DebugSymbolHandle = Arc::new(RwLock::new(Some(resolver)));

        let addr = start_server(serve_dir, 0, None, handle, None)
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let resp = reqwest::get(format!("http://{addr}/sketchsource/src/demo.ino"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let ct = resp
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(ct.contains("text/plain"), "expected text/plain, got {ct}");
        assert!(resp.text().await.unwrap().contains("void loop()"));

        let resp = reqwest::get(format!(
            "http://{addr}/.fastled/cache/fl/repo/sketchsource/src/demo.ino"
        ))
        .await
        .unwrap();
        assert_eq!(resp.status(), 200);
        assert!(resp.text().await.unwrap().contains("void loop()"));
    }

    #[tokio::test]
    async fn source_map_get_works_from_debug_symbol_manifest() {
        use crate::debug_symbols::{
            load_debug_symbol_config, read_debug_symbol_manifest, write_debug_symbol_manifest,
            DebugSymbolResolver,
        };

        let dir = tempfile::tempdir().unwrap();
        let serve_dir = dir.path().join("fastled_js");
        let sketch_dir = dir.path().join("sketch");
        fs::create_dir_all(sketch_dir.join("src")).unwrap();
        fs::create_dir_all(&serve_dir).unwrap();
        fs::write(sketch_dir.join("src").join("demo.ino"), "void setup() {}").unwrap();

        let config = load_debug_symbol_config(sketch_dir, None, None);
        write_debug_symbol_manifest(&serve_dir, &config).unwrap();
        let loaded = read_debug_symbol_manifest(&serve_dir)
            .unwrap()
            .expect("manifest should exist");
        let handle: DebugSymbolHandle =
            Arc::new(RwLock::new(Some(DebugSymbolResolver::new(loaded))));

        let addr = start_server(serve_dir, 0, None, handle, None)
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let resp = reqwest::get(format!("http://{addr}/sketchsource/src/demo.ino"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        assert!(resp.text().await.unwrap().contains("void setup()"));
    }

    #[tokio::test]
    async fn dwarfsource_rejects_traversal() {
        use crate::debug_symbols::{load_debug_symbol_config, DebugSymbolResolver};

        let dir = tempfile::tempdir().unwrap();
        let sketch_dir = dir.path().join("sketch");
        fs::create_dir_all(&sketch_dir).unwrap();
        let resolver = DebugSymbolResolver::new(load_debug_symbol_config(sketch_dir, None, None));
        let handle: DebugSymbolHandle = Arc::new(RwLock::new(Some(resolver)));

        let addr = start_server(dir.path().to_path_buf(), 0, None, handle, None)
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let resp = post_json(
            addr,
            "/dwarfsource",
            serde_json::json!({"path": "sketchsource/../escape.txt"}),
        )
        .await;
        assert_eq!(resp.status(), 400);
    }
}
