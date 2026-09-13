//! Embedded HTTP/HTTPS static file server.
//!
//! Serves compiled FastLED output (JS, WASM, HTML) with the correct
//! COOP/COEP headers for SharedArrayBuffer support.  When index.html
//! does not exist yet (compilation in progress) a built-in loading page
//! is returned that polls `/build-status.json` for live updates.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use kernal_api::{
    async_engine,
    http_server::{Limits, Request, Response, Server},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

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
    pub(crate) events: async_engine::UnboundedSender<TestEvent>,
    pub(crate) token: String,
    pub(crate) sleep_permits: async_engine::Semaphore,
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
      if (l.includes('warning:') || stream === 'warning') { return 'warn'; }
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
    build_tx: Option<async_engine::BroadcastSender<String>>,
    /// Shared resolver for DWARF source paths. Empty until a successful
    /// build populates it.
    debug_symbols: DebugSymbolHandle,
    test: Option<Arc<TestServerOptions>>,
    screenshot_io: kernal_api::platform::fs::AsyncFileIo,
    file_responses: kernal_api::http_server::FileResponses,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

type Reply = std::io::Result<Response>;

fn empty(status: u16) -> Reply {
    Response::new(status, Vec::new())
}

fn text(status: u16, body: impl Into<String>) -> Reply {
    Response::new(status, body.into().into_bytes())?
        .with_header("content-type", "text/plain; charset=utf-8")
}

fn json_reply(status: u16, value: impl Serialize) -> Reply {
    Response::new(
        status,
        serde_json::to_vec(&value).map_err(std::io::Error::other)?,
    )?
    .with_header("content-type", "application/json")
}

async fn stream_file(state: &AppState, path: &std::path::Path, prefix: &[u8], mime: &str) -> Reply {
    state
        .file_responses
        .open(path.to_path_buf(), prefix.to_vec())
        .await?
        .with_header("content-type", mime)
}

async fn serve_index(state: &AppState) -> Reply {
    let index = state.serve_dir.join("index.html");
    if index.is_file() {
        stream_file(state, &index, &[], "text/html; charset=utf-8").await
    } else {
        Response::new(200, LOADING_PAGE.as_bytes().to_vec())?
            .with_header("content-type", "text/html; charset=utf-8")
    }
}

async fn serve_file(state: &AppState, path: &str) -> Reply {
    let file_path = state.serve_dir.join(path);
    let canonical = match file_path.canonicalize() {
        Ok(path) => path,
        Err(_) => {
            return serve_debug_source_url(state, path)
                .await
                .unwrap_or_else(|| empty(404))
        }
    };
    let serve_canonical = state.serve_dir.canonicalize()?;
    if !canonical.starts_with(&serve_canonical) {
        return empty(403);
    }
    let prefix = if state.test.is_some() && path == "fastled_background_worker.js" {
        TEST_WORKER_WEBGL_PREFIX.as_bytes()
    } else {
        &[]
    };
    match stream_file(state, &canonical, prefix, mime_for_path(path)).await {
        Ok(response) => Ok(response),
        Err(_) => serve_debug_source_url(state, path)
            .await
            .unwrap_or_else(|| empty(404)),
    }
}

async fn serve_debug_source_url(state: &AppState, path: &str) -> Option<Reply> {
    let resolver = state.debug_symbols.read().ok()?.clone()?;
    match resolver.resolve(path, true) {
        Ok(file) => Some(stream_file(state, &file, &[], "text/plain; charset=utf-8").await),
        Err(ResolveError::NotFound(_)) => Some(empty(404)),
        Err(ResolveError::Invalid(_)) => None,
    }
}

fn build_stream(state: &AppState) -> Reply {
    let Some(tx) = &state.build_tx else {
        return empty(404);
    };
    let stream = tx
        .subscribe()
        .into_stream_with(|result| result.ok().map(Ok));
    Response::event_stream(stream, std::time::Duration::from_secs(15))
}

fn test_request_authorized(test: &TestServerOptions, request: &Request) -> bool {
    request
        .header("authorization")
        .and_then(|value| std::str::from_utf8(value).ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|value| value == test.token)
}

fn required_query(request: &Request, name: &str) -> std::io::Result<String> {
    let mut value = None;
    for pair in request.query_pairs() {
        let (key, candidate) = pair?;
        if key == name && value.replace(candidate).is_some() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "duplicate query field",
            ));
        }
    }
    value
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing query field"))
}

async fn test_sleep(state: &AppState, request: &Request) -> Reply {
    let ms = match required_query(request, "ms")
        .and_then(|value| value.parse::<f64>().map_err(std::io::Error::other))
    {
        Ok(ms) => ms,
        Err(error) => return text(400, error.to_string()),
    };
    let Some(test) = &state.test else {
        return empty(404);
    };
    if !test_request_authorized(test, request) {
        return empty(401);
    }
    let Ok(duration) = std::time::Duration::try_from_secs_f64(ms / 1000.0) else {
        return text(400, "invalid sleep duration");
    };
    let max_ms = test
        .runtime
        .interval_ms
        .unwrap_or(0.0)
        .max(test.runtime.wait_ms);
    if ms > max_ms {
        return text(400, "sleep exceeds test schedule");
    }
    let Some(_permit) = test.sleep_permits.try_acquire() else {
        return empty(429);
    };
    async_engine::sleep(duration).await;
    empty(204)
}

async fn viewer_screenshot(state: &AppState, request: &Request) -> Reply {
    let name = match required_query(request, "name") {
        Ok(name) => name,
        Err(error) => return text(400, error.to_string()),
    };
    let Some(test) = &state.test else {
        return empty(404);
    };
    if !test_request_authorized(test, request) {
        return empty(401);
    }
    let Some(path) = test.screenshot_paths.get(&name) else {
        return text(400, "unknown screenshot name");
    };
    let body = request.body();
    if body.len() < 8 || body[..8] != [137, 80, 78, 71, 13, 10, 26, 10] {
        let message = format!("viewer returned invalid PNG data for {}", path.display());
        let _ = test.events.send(TestEvent::Failure(message.clone()));
        return text(400, message);
    }
    if let Err(error) = state.screenshot_io.write(path.clone(), body.to_vec()).await {
        let message = format!("could not write screenshot {}: {error}", path.display());
        let _ = test.events.send(TestEvent::Failure(message.clone()));
        return text(500, message);
    }
    let _ = test.events.send(TestEvent::ScreenshotSaved {
        name,
        path: path.clone(),
    });
    empty(204)
}

#[derive(Deserialize)]
struct DwarfSourceRequest {
    path: String,
}

async fn dwarf_source(state: &AppState, request: &Request) -> Reply {
    let content_type = request
        .header("content-type")
        .and_then(|value| std::str::from_utf8(value).ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if content_type != "application/json"
        && !(content_type.starts_with("application/") && content_type.ends_with("+json"))
    {
        return text(415, "expected application/json");
    }
    let payload = match serde_json::from_slice::<DwarfSourceRequest>(request.body()) {
        Ok(payload) => payload,
        Err(error) => return text(if error.is_data() { 422 } else { 400 }, error.to_string()),
    };
    let resolver = state
        .debug_symbols
        .read()
        .map_err(|_| std::io::Error::other("debug source lock poisoned"))?
        .clone();
    let Some(resolver) = resolver else {
        return json_reply(400, json!({"error": "debug source resolver unavailable"}));
    };
    let path = payload.path.trim();
    if path.is_empty() {
        return json_reply(400, json!({"error": "missing path"}));
    }
    match resolver.resolve(path, true) {
        Ok(file) => match stream_file(state, &file, &[], "text/plain; charset=utf-8").await {
            Ok(response) => Ok(response),
            Err(error) => json_reply(500, json!({"error": error.to_string()})),
        },
        Err(ResolveError::NotFound(message)) => json_reply(404, json!({"error": message})),
        Err(ResolveError::Invalid(message)) => json_reply(400, json!({"error": message})),
    }
}

fn debug_source_roots(state: &AppState) -> Reply {
    let resolver = state
        .debug_symbols
        .read()
        .map_err(|_| std::io::Error::other("debug source lock poisoned"))?
        .clone();
    let roots =
        resolver
            .map(|resolver| {
                resolver.config().source_roots().into_iter()
        .map(|(prefix, path)| json!({"prefix": prefix, "path": path.display().to_string()}))
        .collect::<Vec<_>>()
            })
            .unwrap_or_default();
    json_reply(200, json!({"roots": roots}))
}

async fn route(state: &AppState, request: Request) -> Reply {
    if request.method() == "OPTIONS" {
        return empty(200)?
            .with_header(
                "access-control-allow-methods",
                "GET,POST,PUT,DELETE,OPTIONS",
            )?
            .with_header("access-control-allow-headers", "content-type,authorization");
    }
    let path = match request.decoded_path() {
        Ok(path) => path,
        Err(error) => return text(400, error.to_string()),
    };
    let post = matches!(
        path.as_str(),
        "/dwarfsource"
            | "/viewer-log"
            | "/test-ready"
            | "/test-sleep"
            | "/viewer-screenshot"
            | "/test-done"
    );
    if (post && request.method() != "POST")
        || (!post && !matches!(request.method(), "GET" | "HEAD"))
    {
        return empty(405)?.with_header("allow", if post { "POST" } else { "GET,HEAD" });
    }
    match path.as_str() {
        "/" => serve_index(state).await,
        "/build-stream" => build_stream(state),
        "/dwarfsource" => dwarf_source(state, &request).await,
        "/debug/source-roots" => debug_source_roots(state),
        "/test-sleep" => test_sleep(state, &request).await,
        "/viewer-screenshot" => viewer_screenshot(state, &request).await,
        "/test-config" | "/test-ready" | "/test-done" | "/viewer-log" => {
            let body = if matches!(path.as_str(), "/test-done" | "/viewer-log") {
                match std::str::from_utf8(request.body()) {
                    Ok(body) => body,
                    Err(_) => return text(400, "invalid UTF-8 body"),
                }
            } else {
                ""
            };
            if let Some(test) = &state.test {
                if !test_request_authorized(test, &request) {
                    return empty(401);
                }
                match path.as_str() {
                    "/test-config" => return json_reply(200, &test.runtime),
                    "/test-ready" => {
                        let _ = test.events.send(TestEvent::Ready);
                    }
                    "/test-done" => {
                        let _ = test
                            .events
                            .send(TestEvent::Done(body.trim().parse::<u8>().unwrap_or(1)));
                    }
                    _ => {}
                }
            } else if path != "/viewer-log" {
                return empty(404);
            }
            if path == "/viewer-log" {
                for line in body.lines() {
                    eprintln!("[viewer] {line}");
                    if let Some(test) = &state.test {
                        let _ = test.events.send(TestEvent::ViewerLog(line.to_string()));
                    }
                }
            }
            empty(204)
        }
        _ => serve_file(state, path.strip_prefix('/').unwrap_or(&path)).await,
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

fn server_limits(runtime: Option<&TestRuntimeConfig>) -> std::io::Result<Limits> {
    let schedule = runtime
        .map(|runtime| runtime.wait_ms.max(runtime.interval_ms.unwrap_or(0.0)))
        .unwrap_or(0.0);
    let wait =
        std::time::Duration::try_from_secs_f64(schedule / 1000.0).map_err(std::io::Error::other)?;
    let deadline = wait
        .checked_add(std::time::Duration::from_secs(60))
        .ok_or_else(|| std::io::Error::other("test schedule deadline overflow"))?
        .max(std::time::Duration::from_secs(3600));
    Ok(Limits {
        max_request_body_bytes: 64 * 1024 * 1024,
        handler_timeout: deadline,
        connection_timeout: deadline,
        ..Limits::default()
    })
}

/// Start the kernel HTTP server in a caller-runtime-owned background task.
///
/// Returns the actual address the server bound to (useful when port 0 is
/// requested for automatic assignment). `debug_symbols` is shared with the
/// caller so a build can populate the resolver after the server starts.
pub async fn start_server(
    serve_dir: PathBuf,
    port: u16,
    build_tx: Option<async_engine::BroadcastSender<String>>,
    debug_symbols: DebugSymbolHandle,
    test: Option<TestServerOptions>,
) -> anyhow::Result<SocketAddr> {
    let state = AppState {
        serve_dir: Arc::new(serve_dir),
        build_tx,
        debug_symbols,
        test: test.map(Arc::new),
        // One shared budget per server, including native writes still completing
        // after timeout. PNG validation and destination policy stay in this app.
        file_responses: kernal_api::http_server::FileResponses::new(
            4,
            std::time::Duration::from_secs(30),
        )?,
        screenshot_io: kernal_api::platform::fs::AsyncFileIo::new(
            4,
            64 * 1024 * 1024,
            std::time::Duration::from_secs(30),
        )?,
    };

    let server = Server::bind(
        SocketAddr::from(([127, 0, 0, 1], port)),
        server_limits(state.test.as_ref().map(|test| &test.runtime))?,
    )
    .await?
    .with_response_header("cross-origin-embedder-policy", "require-corp")?
    .with_response_header("cross-origin-opener-policy", "same-origin")?
    .with_response_header("cache-control", "no-cache, no-store, must-revalidate")?
    .with_response_header("access-control-allow-origin", "*")?
    .with_response_header(
        "vary",
        "origin, access-control-request-method, access-control-request-headers",
    )?;
    let addr = server.local_addr()?;
    let diagnostics = server.diagnostics();
    async_engine::launch(async move {
        let result = server
            .serve(move |request| {
                let state = state.clone();
                async move {
                    match route(&state, request).await {
                        Ok(response) => response,
                        Err(error) => {
                            eprintln!("[server] response preparation failed: {error}");
                            Response::default()
                        }
                    }
                }
            })
            .await;
        if let Err(error) = result {
            eprintln!("[server] listener failed: {error}");
        }
        eprintln!("[server] stopped: {:?}", diagnostics.snapshot());
    })
    .detach();

    Ok(addr)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use kernal_api::http::{
        Client as HttpClient, Limits as HttpLimits, Method as HttpMethod, Request as HttpRequest,
        Response as HttpResponse,
    };
    use std::fs;

    async fn http_request(
        method: HttpMethod,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> std::io::Result<HttpResponse> {
        HttpClient::new(HttpLimits::default())?
            .execute(HttpRequest {
                method,
                url,
                headers,
                body,
            })
            .await
    }

    async fn http_get(url: String) -> std::io::Result<HttpResponse> {
        http_request(HttpMethod::Get, &url, &[], &[]).await
    }

    async fn response_text(response: HttpResponse) -> String {
        String::from_utf8(response.into_bytes().await.unwrap()).unwrap()
    }

    fn header_text<'a>(response: &'a HttpResponse, name: &str) -> &'a str {
        std::str::from_utf8(response.header(name).unwrap()).unwrap()
    }

    fn empty_handle() -> DebugSymbolHandle {
        Arc::new(RwLock::new(None))
    }

    #[test]
    fn server_deadlines_cover_every_accepted_test_sleep() {
        for ms in [25.0, 3_600_001.0, f64::from(i32::MAX)] {
            let runtime = TestRuntimeConfig {
                wait_ms: ms,
                interval_ms: Some(ms),
                screenshot_names: Vec::new(),
            };
            let limits = server_limits(Some(&runtime)).unwrap();
            let wait = std::time::Duration::from_secs_f64(ms / 1000.0);
            assert!(limits.handler_timeout > wait);
            assert!(limits.connection_timeout > wait);
            assert_eq!(limits.handler_timeout, limits.connection_timeout);
        }
    }

    /// Helper: create a temp dir, start the server, return (addr, dir).
    async fn setup_server() -> (SocketAddr, kernal_api::platform::fs::TemporaryDirectory) {
        let dir = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        let addr = start_server(dir.path().to_path_buf(), 0, None, empty_handle(), None)
            .await
            .unwrap();
        // Give the server a moment to bind.
        async_engine::sleep(std::time::Duration::from_millis(50)).await;
        (addr, dir)
    }

    async fn raw_http(addr: SocketAddr, request: String) -> String {
        async_engine::launch_blocking(move || {
            use std::io::{Read, Write};
            let mut socket =
                std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(3))
                    .unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            socket
                .set_write_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            socket.write_all(request.as_bytes()).unwrap();
            let mut bytes = Vec::new();
            socket.read_to_end(&mut bytes).unwrap();
            String::from_utf8(bytes).unwrap()
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn http_router_migration_preserves_head_errors_and_preflight_policy() {
        let (addr, dir) = setup_server().await;
        fs::write(dir.path().join("asset.js"), "hello").unwrap();
        for (method, path, status) in [
            ("GET", "/missing", 404),
            ("HEAD", "/asset.js", 200),
            ("POST", "/asset.js", 405),
        ] {
            let response = raw_http(addr, format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nOrigin: http://example.test\r\nConnection: close\r\nContent-Length: 0\r\n\r\n")).await;
            assert!(
                response.starts_with(&format!("HTTP/1.1 {status}")),
                "{response}"
            );
            for header in [
                "cross-origin-embedder-policy: require-corp\r\n",
                "cross-origin-opener-policy: same-origin\r\n",
                "cache-control: no-cache, no-store, must-revalidate\r\n",
                "access-control-allow-origin: *\r\n",
            ] {
                assert!(response.contains(header), "missing {header:?}: {response}");
            }
            if method == "HEAD" {
                assert!(response.contains("content-length: 5\r\n"), "{response}");
                assert!(response.ends_with("\r\n\r\n"), "{response}");
            }
        }
        let response = raw_http(addr, "OPTIONS /viewer-screenshot HTTP/1.1\r\nHost: localhost\r\nOrigin: http://example.test\r\nAccess-Control-Request-Method: POST\r\nAccess-Control-Request-Headers: authorization,content-type\r\nConnection: close\r\n\r\n".into()).await;
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(
            response.contains("access-control-allow-origin: *\r\n"),
            "{response}"
        );
        assert!(
            response.contains("access-control-allow-methods: GET,POST,PUT,DELETE,OPTIONS\r\n"),
            "{response}"
        );
        assert!(
            response.contains("access-control-allow-headers: content-type,authorization\r\n"),
            "{response}"
        );
        assert!(response.ends_with("\r\n\r\n"), "{response}");
    }

    #[tokio::test]
    async fn malformed_requests_respect_the_browser_header_dispatch_boundary() {
        let (addr, _dir) = setup_server().await;
        let parser_error = raw_http(
            addr,
            "GET / HTTP/1.1\r\nHost: localhost\r\nContent-Length: invalid\r\nConnection: close\r\n\r\n".into(),
        )
        .await;
        assert!(parser_error.starts_with("HTTP/1.1 400"), "{parser_error}");
        assert!(!parser_error.contains("cross-origin-opener-policy:"));
        assert!(!parser_error.contains("access-control-allow-origin:"));

        for target in ["/%zz", "/test-sleep?ms=%zz"] {
            let response = raw_http(
                addr,
                format!("POST {target} HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
            )
            .await;
            assert!(response.starts_with("HTTP/1.1 400"), "{response}");
            assert!(response.contains("cross-origin-opener-policy: same-origin\r\n"));
            assert!(response.contains("cross-origin-embedder-policy: require-corp\r\n"));
            assert!(response.contains("access-control-allow-origin: *\r\n"));
            assert!(response.contains("cache-control: no-cache, no-store, must-revalidate\r\n"));
        }
    }

    #[tokio::test]
    async fn test_viewer_log_endpoint_accepts_posts() {
        let (addr, _dir) = setup_server().await;
        let resp = http_request(
            HttpMethod::Post,
            &format!("http://{addr}/viewer-log"),
            &[],
            b"error: something broke",
        )
        .await
        .unwrap();
        assert_eq!(resp.status(), 204);
    }

    #[tokio::test]
    async fn test_runtime_endpoints_use_preconfigured_screenshot_paths() {
        let dir = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        fs::write(
            dir.path().join("fastled_background_worker.js"),
            "console.log('worker');",
        )
        .unwrap();
        let screenshot = dir.path().join("artifacts").join("frame.png");
        let (events, mut rx) = async_engine::unbounded_channel();
        let sleep_permits = async_engine::Semaphore::new(4);
        let options = TestServerOptions {
            runtime: TestRuntimeConfig {
                wait_ms: 25.0,
                interval_ms: Some(10.0),
                screenshot_names: vec!["frame-0".to_string()],
            },
            screenshot_paths: HashMap::from([("frame-0".to_string(), screenshot.clone())]),
            events,
            token: "test-token".to_string(),
            sleep_permits: sleep_permits.clone(),
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

        let unauthorized = http_request(
            HttpMethod::Get,
            &format!("http://{addr}/test-config"),
            &[],
            &[],
        )
        .await
        .unwrap();
        assert_eq!(unauthorized.status(), 401);
        let config_response = http_request(
            HttpMethod::Get,
            &format!("http://{addr}/test-config"),
            &[("Authorization", "Bearer test-token")],
            &[],
        )
        .await
        .unwrap();
        let config: serde_json::Value =
            serde_json::from_str(&response_text(config_response).await).unwrap();
        assert_eq!(config["waitMs"].as_f64(), Some(25.0));
        assert_eq!(config["screenshotNames"][0], "frame-0");

        // Product policy: four occupied slots reject another authorized sleep
        // with HTTP 429; releasing a slot admits the next request.
        let mut occupied: Vec<_> = (0..4)
            .map(|_| sleep_permits.try_acquire().unwrap())
            .collect();
        let saturated = http_request(
            HttpMethod::Post,
            &format!("http://{addr}/test-sleep?ms=1"),
            &[("Authorization", "Bearer test-token")],
            &[],
        )
        .await
        .unwrap();
        assert_eq!(saturated.status(), 429);
        drop(occupied.pop());
        let response = http_request(
            HttpMethod::Post,
            &format!("http://{addr}/test-sleep?ms=1"),
            &[("Authorization", "Bearer test-token")],
            &[],
        )
        .await
        .unwrap();
        assert_eq!(response.status(), 204);
        assert_eq!(sleep_permits.available_permits(), 1);
        drop(occupied);
        let response = http_request(
            HttpMethod::Post,
            &format!("http://{addr}/test-sleep?ms=-1"),
            &[("Authorization", "Bearer test-token")],
            &[],
        )
        .await
        .unwrap();
        assert_eq!(response.status(), 400);
        let response = http_request(
            HttpMethod::Post,
            &format!("http://{addr}/test-sleep?ms=26"),
            &[("Authorization", "Bearer test-token")],
            &[],
        )
        .await
        .unwrap();
        assert_eq!(response.status(), 400);

        let worker = response_text(
            http_get(format!("http://{addr}/fastled_background_worker.js"))
                .await
                .unwrap(),
        )
        .await;
        assert!(worker.starts_with("\nconst __fastledOriginalOffscreenGetContext"));
        assert!(worker.contains("preserveDrawingBuffer: true"));
        assert!(worker.contains("console.log('worker');"));
        assert!(worker.contains("gl.readPixels"));
        assert!(worker.contains("fastled_test_capture_response"));

        let response = http_request(
            HttpMethod::Post,
            &format!("http://{addr}/viewer-screenshot?name=..%2Fescape"),
            &[("Authorization", "Bearer test-token")],
            &[137, 80, 78, 71, 13, 10, 26, 10],
        )
        .await
        .unwrap();
        assert_eq!(response.status(), 400);
        assert!(!dir.path().join("escape").exists());

        let png = vec![137, 80, 78, 71, 13, 10, 26, 10, 1, 2, 3];
        let response = http_request(
            HttpMethod::Post,
            &format!("http://{addr}/viewer-screenshot?name=frame-0"),
            &[("Authorization", "Bearer test-token")],
            &png,
        )
        .await
        .unwrap();
        assert_eq!(response.status(), 204);
        assert_eq!(fs::read(&screenshot).unwrap(), png);
        assert!(matches!(
            rx.recv().await,
            Some(TestEvent::ScreenshotSaved { name, .. }) if name == "frame-0"
        ));

        // A persistence error must remain a product failure, never a saved
        // event. Turn this test's temporary output into a directory to force it.
        fs::remove_file(&screenshot).unwrap();
        fs::create_dir(&screenshot).unwrap();
        let response = http_request(
            HttpMethod::Post,
            &format!("http://{addr}/viewer-screenshot?name=frame-0"),
            &[("Authorization", "Bearer test-token")],
            &png,
        )
        .await
        .unwrap();
        assert_eq!(response.status(), 500);
        assert!(matches!(
            rx.recv().await,
            Some(TestEvent::Failure(message)) if message.contains("could not write screenshot")
        ));
    }

    #[tokio::test]
    async fn test_loading_page_when_no_index_html() {
        let (addr, _dir) = setup_server().await;
        let resp = http_get(format!("http://{addr}/")).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = response_text(resp).await;
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
        let resp = http_get(format!("http://{addr}/")).await.unwrap();
        assert_eq!(
            resp.status(),
            200,
            "viewer should land on the loading page, not 404/redirect, when compile failed"
        );
        let body = response_text(resp).await;
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
        let resp = http_get(format!("http://{addr}/")).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = response_text(resp).await;
        assert!(
            body.contains("OK"),
            "expected index.html content, got: {body}"
        );
    }

    #[tokio::test]
    async fn test_serves_js_with_correct_mime() {
        let (addr, dir) = setup_server().await;
        fs::write(dir.path().join("app.js"), "console.log('hi')").unwrap();
        let resp = http_get(format!("http://{addr}/app.js")).await.unwrap();
        assert_eq!(resp.status(), 200);
        let ct = header_text(&resp, "content-type");
        assert!(ct.contains("javascript"), "expected JS mime, got: {ct}");
    }

    #[tokio::test]
    async fn test_serves_wasm_with_correct_mime() {
        let (addr, dir) = setup_server().await;
        fs::write(dir.path().join("fastled.wasm"), [0x00, 0x61, 0x73, 0x6d]).unwrap();
        let resp = http_get(format!("http://{addr}/fastled.wasm"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let ct = header_text(&resp, "content-type");
        assert!(ct.contains("wasm"), "expected WASM mime, got: {ct}");
    }

    #[tokio::test]
    async fn test_safari_compatible_coop_coep_headers() {
        let (addr, _dir) = setup_server().await;
        let resp = http_get(format!("http://{addr}/")).await.unwrap();
        let coep = header_text(&resp, "cross-origin-embedder-policy");
        let coop = header_text(&resp, "cross-origin-opener-policy");
        assert_eq!(coep, "require-corp");
        assert_eq!(coop, "same-origin");
    }

    #[tokio::test]
    async fn test_404_for_missing_file() {
        let (addr, _dir) = setup_server().await;
        let resp = http_get(format!("http://{addr}/nonexistent.js"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 404);
    }

    #[tokio::test]
    async fn test_build_status_json_served() {
        let (addr, dir) = setup_server().await;
        // Initially no build-status.json -> 404
        let resp = http_get(format!("http://{addr}/build-status.json"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 404);

        // Write status file -> 200
        fs::write(
            dir.path().join("build-status.json"),
            r#"{"status":"compiling","message":"Building..."}"#,
        )
        .unwrap();
        let resp = http_get(format!("http://{addr}/build-status.json"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body = response_text(resp).await;
        assert!(body.contains("compiling"));
    }

    #[tokio::test]
    async fn test_directory_traversal_blocked() {
        let (addr, dir) = setup_server().await;
        // Create a file outside the serve dir
        let parent = dir.path().parent().unwrap();
        fs::write(parent.join("secret.txt"), "top secret").unwrap();
        let resp = http_get(format!("http://{addr}/../secret.txt"))
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
        let resp = http_get(format!("http://{addr}/build-stream"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 404);
    }

    #[tokio::test]
    async fn test_sse_endpoint_streams_events() {
        let dir = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        let (tx, _rx) = async_engine::broadcast_channel::<String>(16).unwrap();
        let addr = start_server(
            dir.path().to_path_buf(),
            0,
            Some(tx.clone()),
            empty_handle(),
            None,
        )
        .await
        .unwrap();
        async_engine::sleep(std::time::Duration::from_millis(50)).await;

        let url = format!("http://{addr}/build-stream");

        // Connect to SSE endpoint.
        let mut resp = http_get(url).await.unwrap();
        assert_eq!(resp.status(), 200);
        let ct = header_text(&resp, "content-type").to_string();
        assert!(
            ct.contains("text/event-stream"),
            "expected event-stream content-type, got: {ct}"
        );

        // Give the server handler a moment to subscribe to the broadcast.
        async_engine::sleep(std::time::Duration::from_millis(50)).await;

        // Send test events.
        tx.send(r#"{"type":"log","line":"Building sketch...","stream":"stdout"}"#.to_string())
            .unwrap();
        tx.send(r#"{"type":"status","status":"success","message":"Done"}"#.to_string())
            .unwrap();

        // Read SSE chunks until we see both events (or timeout).
        let mut collected = String::new();
        let deadline = std::time::Duration::from_secs(3);
        let mut buffer = [0; 4096];
        while let Ok(Ok(n)) = async_engine::timeout(deadline, resp.read(&mut buffer)).await {
            if n == 0 {
                break;
            }
            collected.push_str(&String::from_utf8_lossy(&buffer[..n]));
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
        let resp = http_get(format!("http://{addr}/")).await.unwrap();
        let body = response_text(resp).await;
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

    async fn post_json(addr: SocketAddr, path: &str, body: serde_json::Value) -> HttpResponse {
        http_request(
            HttpMethod::Post,
            &format!("http://{addr}{path}"),
            &[("Content-Type", "application/json")],
            json_body(body).as_bytes(),
        )
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
        let resp = http_get(format!("http://{addr}/debug/source-roots"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body: serde_json::Value = serde_json::from_str(&response_text(resp).await).unwrap();
        assert!(body["roots"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn dwarfsource_returns_resolved_file() {
        use crate::debug_symbols::{load_debug_symbol_config, DebugSymbolResolver};

        let dir = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        let sketch_dir = dir.path().join("sketch");
        fs::create_dir_all(sketch_dir.join("src")).unwrap();
        let sketch_file = sketch_dir.join("src").join("demo.ino");
        fs::write(&sketch_file, "void setup() {}").unwrap();

        let resolver = DebugSymbolResolver::new(load_debug_symbol_config(sketch_dir, None, None));
        let handle: DebugSymbolHandle = Arc::new(RwLock::new(Some(resolver)));

        let addr = start_server(dir.path().to_path_buf(), 0, None, handle.clone(), None)
            .await
            .unwrap();
        async_engine::sleep(std::time::Duration::from_millis(50)).await;

        let resp = post_json(
            addr,
            "/dwarfsource",
            serde_json::json!({"path": "sketchsource/src/demo.ino"}),
        )
        .await;
        assert_eq!(resp.status(), 200);
        let body = response_text(resp).await;
        assert!(body.contains("void setup()"));

        let resp = http_get(format!("http://{addr}/debug/source-roots"))
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_str(&response_text(resp).await).unwrap();
        let roots = body["roots"].as_array().unwrap();
        assert!(!roots.is_empty());
        assert!(roots
            .iter()
            .any(|r| r["prefix"].as_str() == Some("sketchsource")));
    }

    #[tokio::test]
    async fn source_map_style_get_returns_resolved_file() {
        use crate::debug_symbols::{load_debug_symbol_config, DebugSymbolResolver};

        let dir = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
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
        async_engine::sleep(std::time::Duration::from_millis(50)).await;

        let resp = http_get(format!("http://{addr}/sketchsource/src/demo.ino"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let ct = header_text(&resp, "content-type");
        assert!(ct.contains("text/plain"), "expected text/plain, got {ct}");
        assert!(response_text(resp).await.contains("void loop()"));

        let resp = http_get(format!(
            "http://{addr}/.fastled/cache/fl/repo/sketchsource/src/demo.ino"
        ))
        .await
        .unwrap();
        assert_eq!(resp.status(), 200);
        assert!(response_text(resp).await.contains("void loop()"));
    }

    #[tokio::test]
    async fn source_map_get_works_from_debug_symbol_manifest() {
        use crate::debug_symbols::{
            load_debug_symbol_config, read_debug_symbol_manifest, write_debug_symbol_manifest,
            DebugSymbolResolver,
        };

        let dir = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
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
        async_engine::sleep(std::time::Duration::from_millis(50)).await;

        let resp = http_get(format!("http://{addr}/sketchsource/src/demo.ino"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        assert!(response_text(resp).await.contains("void setup()"));
    }

    #[tokio::test]
    async fn dwarfsource_rejects_traversal() {
        use crate::debug_symbols::{load_debug_symbol_config, DebugSymbolResolver};

        let dir = kernal_api::platform::fs::TemporaryDirectory::new().unwrap();
        let sketch_dir = dir.path().join("sketch");
        fs::create_dir_all(&sketch_dir).unwrap();
        let resolver = DebugSymbolResolver::new(load_debug_symbol_config(sketch_dir, None, None));
        let handle: DebugSymbolHandle = Arc::new(RwLock::new(Some(resolver)));

        let addr = start_server(dir.path().to_path_buf(), 0, None, handle, None)
            .await
            .unwrap();
        async_engine::sleep(std::time::Duration::from_millis(50)).await;

        let resp = post_json(
            addr,
            "/dwarfsource",
            serde_json::json!({"path": "sketchsource/../escape.txt"}),
        )
        .await;
        assert_eq!(resp.status(), 400);
    }
}
