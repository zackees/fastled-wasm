use std::process::ExitCode;

pub struct ViewerOptions {
    pub url: String,
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub inject_test_runtime: bool,
}

const TEST_CAPABILITY_SCRIPT: &str = r#"
(() => {
  const params = new URLSearchParams(location.hash.slice(1));
  const token = params.get('fastled-test-token');
  if (token) {
    window.__fastled_test_token = token;
    history.replaceState(null, '', location.pathname + location.search);
  }
})();
"#;

/// Injected when `FASTLED_VIEWER_LOGS` is enabled: forwards `console.*`,
/// uncaught errors, unhandled rejections, and failed fetches to the FastLED
/// HTTP server's `POST /viewer-log` endpoint, which echoes them to stderr.
const LOG_FORWARD_SCRIPT: &str = r#"
(() => {
  const pending = window.__fastled_test_pending_logs = new Set();
  const send = (line) => {
    try {
      const headers = window.__fastled_test_token
        ? { 'Authorization': 'Bearer ' + window.__fastled_test_token }
        : {};
      const request = fetch('/viewer-log', { method: 'POST', headers, body: line }).catch(() => {});
      pending.add(request);
      request.finally(() => pending.delete(request));
    } catch (_) {}
  };
  const fmt = (args) => Array.from(args).map((a) => {
    if (a instanceof Error) return a.stack || String(a);
    if (typeof a === 'object' && a !== null) {
      try { return JSON.stringify(a); } catch (_) { return String(a); }
    }
    return String(a);
  }).join(' ');
  for (const level of ['log', 'info', 'warn', 'error', 'debug']) {
    const original = console[level].bind(console);
    console[level] = (...args) => { original(...args); send(level + ': ' + fmt(args)); };
  }
  window.addEventListener('error', (e) => {
    send('window.onerror: ' + e.message + ' (' + e.filename + ':' + e.lineno + ')');
  });
  window.addEventListener('unhandledrejection', (e) => {
    send('unhandledrejection: ' + fmt([e.reason]));
  });
  const originalFetch = window.fetch.bind(window);
  window.fetch = async (...args) => {
    const url = String(args[0] && args[0].url ? args[0].url : args[0]);
    if (url.endsWith('/viewer-log')) return originalFetch(...args);
    try {
      const resp = await originalFetch(...args);
      if (!resp.ok) send('fetch failed: ' + resp.status + ' ' + url);
      return resp;
    } catch (err) {
      send('fetch error: ' + url + ' ' + String(err));
      throw err;
    }
  };
})();
"#;

/// Runs before page JavaScript. It forces WebGL drawing-buffer preservation,
/// waits for a real rendered canvas, then follows the host-provided schedule.
const TEST_RUNTIME_SCRIPT: &str = r#"
(() => {
  const originalGetContext = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = function(kind, attributes) {
    if (kind === 'webgl' || kind === 'webgl2' || kind === 'experimental-webgl') {
      attributes = Object.assign({}, attributes || {}, { preserveDrawingBuffer: true });
    }
    return originalGetContext.call(this, kind, attributes);
  };

  window.__fastled_test = window.__fastled_test || { version: 1 };
  const testFetch = (url, options = {}) => fetch(url, {
    ...options,
    headers: {
      ...(options.headers || {}),
      'Authorization': 'Bearer ' + window.__fastled_test_token
    }
  });
  const hostSleep = async (ms) => {
    if (ms <= 0) return;
    const response = await testFetch('/test-sleep?ms=' + encodeURIComponent(ms), { method: 'POST' });
    if (!response.ok) throw new Error('test sleep failed: ' + response.status);
  };
  const postDone = async (code) => {
    const pending = window.__fastled_test_pending_logs;
    if (pending) await Promise.allSettled(Array.from(pending));
    let lastError = new Error('test completion request failed');
    for (let attempt = 0; attempt < 2; attempt += 1) {
      try {
        const response = await testFetch('/test-done', { method: 'POST', body: String(code) });
        if (response.ok) return;
        lastError = new Error('test completion request failed: ' + response.status);
      } catch (error) {
        lastError = error instanceof Error ? error : new Error(String(error));
      }
    }
    window.close();
    throw lastError;
  };
  const workerFrameDataUrl = async () => {
    const manager = window.fastLEDWorkerManager;
    if (!manager || !manager.worker) return null;
    if (!manager.isWorkerActive) throw new Error('FastLED render worker is not ready');
    let bitmap = null;
    let resolveFrame;
    let rejectFrame;
    const frame = new Promise((resolve, reject) => {
      resolveFrame = resolve;
      rejectFrame = reject;
    });
    const timeout = setTimeout(() => rejectFrame(new Error('worker screenshot frame timed out')), 3000);
    const onMessage = (event) => {
      if (event.data && event.data.type === 'frame_update' && event.data.payload?.bitmap) {
        resolveFrame(event.data.payload);
      }
    };
    manager.worker.addEventListener('message', onMessage);
    try {
      const response = await manager.sendMessageWithResponse({
        type: 'start_recording',
        payload: { fps: 60, settings: {} }
      });
      if (!response || !response.success) throw new Error('worker screenshot capture did not start');
      const payload = await frame;
      bitmap = payload.bitmap;
      const mirror = document.createElement('canvas');
      mirror.width = payload.width;
      mirror.height = payload.height;
      const context = mirror.getContext('2d');
      if (!context) throw new Error('2D screenshot context unavailable');
      context.drawImage(bitmap, 0, 0);
      return mirror.toDataURL('image/png');
    } finally {
      clearTimeout(timeout);
      manager.worker.removeEventListener('message', onMessage);
      if (bitmap && typeof bitmap.close === 'function') bitmap.close();
      try {
        await manager.sendMessageWithResponse({ type: 'stop_recording', payload: {} });
      } catch (_) {}
    }
  };
  const compositedFrameDataUrl = async (canvas) => {
    if (typeof canvas.captureStream !== 'function') return null;
    const stream = canvas.captureStream(0);
    const track = stream.getVideoTracks()[0];
    if (!track) return null;
    let bitmap = null;
    let video = null;
    try {
      if (typeof ImageCapture !== 'undefined') {
        if (typeof track.requestFrame === 'function') track.requestFrame();
        bitmap = await new ImageCapture(track).grabFrame();
      } else {
        video = document.createElement('video');
        video.muted = true;
        video.playsInline = true;
        video.style.cssText = 'position:fixed;width:1px;height:1px;opacity:0;pointer-events:none';
        video.srcObject = stream;
        document.body.appendChild(video);
        const frameReady = new Promise((resolve, reject) => {
          const timeout = setTimeout(() => reject(new Error('composited screenshot frame timed out')), 3000);
          const done = () => { clearTimeout(timeout); resolve(); };
          if (typeof video.requestVideoFrameCallback === 'function') {
            video.requestVideoFrameCallback(done);
          } else {
            video.addEventListener('loadeddata', done, { once: true });
          }
        });
        await video.play();
        if (typeof track.requestFrame === 'function') track.requestFrame();
        await frameReady;
        bitmap = video;
      }
      const mirror = document.createElement('canvas');
      mirror.width = bitmap.width || bitmap.videoWidth || canvas.width;
      mirror.height = bitmap.height || bitmap.videoHeight || canvas.height;
      const context = mirror.getContext('2d');
      if (!context) throw new Error('2D screenshot context unavailable');
      context.drawImage(bitmap, 0, 0);
      return mirror.toDataURL('image/png');
    } finally {
      if (bitmap && bitmap !== video && typeof bitmap.close === 'function') bitmap.close();
      if (video) video.remove();
      stream.getTracks().forEach((item) => item.stop());
    }
  };
  const webglFrameBlob = async () => {
    const manager = window.fastLEDWorkerManager;
    if (!manager || !manager.worker || !manager.isWorkerActive) return null;
    const id = 'capture-' + Date.now() + '-' + Math.random();
    let resolveCapture;
    let rejectCapture;
    const response = new Promise((resolve, reject) => {
      resolveCapture = resolve;
      rejectCapture = reject;
    });
    const timeout = setTimeout(() => rejectCapture(new Error('WebGL screenshot timed out')), 3000);
    const onMessage = (event) => {
      if (event.data && event.data.type === 'fastled_test_capture_response' && event.data.id === id) {
        resolveCapture(event.data);
      }
    };
    manager.worker.addEventListener('message', onMessage);
    try {
      manager.worker.postMessage({ type: 'fastled_test_capture', id });
      const result = await response;
      if (result.error) throw new Error(result.error);
      console.log('[fastled-test] capture pixels=' + result.stats.width + 'x' + result.stats.height
        + ' nonBlack=' + result.stats.nonBlackPixels + ' varied=' + result.stats.varied);
      return new Blob([result.bytes], { type: 'image/png' });
    } finally {
      clearTimeout(timeout);
      manager.worker.removeEventListener('message', onMessage);
    }
  };
  const capture = async (canvas, name) => {
    let blob = await webglFrameBlob();
    if (!blob) {
      const dataUrl = await compositedFrameDataUrl(canvas)
        || await workerFrameDataUrl()
        || canvas.toDataURL('image/png');
      blob = await (await fetch(dataUrl)).blob();
    }
    const response = await testFetch('/viewer-screenshot?name=' + encodeURIComponent(name), {
      method: 'POST',
      headers: { 'Content-Type': 'application/octet-stream' },
      body: blob
    });
    if (!response.ok) throw new Error('screenshot upload failed: ' + response.status);
  };

  let finding = false;
  let frameSeen = false;
  let frameListenerInstalled = false;
  const findCanvas = () => {
    if (finding) return;
    finding = true;
    const tryFind = () => {
      const canvas = document.getElementById('myCanvas') || document.querySelector('canvas');
      const manager = window.fastLEDWorkerManager;
      if (!frameListenerInstalled && window.fastLEDEvents) {
        frameListenerInstalled = true;
        window.fastLEDEvents.on('frame:rendered', () => { frameSeen = true; });
      }
      if (!canvas || !manager || !manager.isWorkerActive || !frameSeen) {
        setTimeout(tryFind, 50);
        return;
      }
      requestAnimationFrame(() => requestAnimationFrame(async () => {
        try {
          const ready = await testFetch('/test-ready', { method: 'POST' });
          if (!ready.ok) throw new Error('ready signal failed: ' + ready.status);
          const response = await testFetch('/test-config');
          if (!response.ok) throw new Error('test config failed: ' + response.status);
          const config = await response.json();
          const firstCaptureAt = performance.now() + config.waitMs;
          const maxInFlightCaptures = 2;
          const inFlightCaptures = new Set();
          const captureResults = [];
          let firstCaptureFailure = null;
          let signalCaptureFailure;
          const captureFailureSignal = new Promise((resolve) => { signalCaptureFailure = resolve; });
          for (let index = 0; index < config.screenshotNames.length; index += 1) {
            const deadline = firstCaptureAt + index * config.intervalMs;
            const scheduledWait = hostSleep(Math.max(0, deadline - performance.now())).then(
              () => null,
              (error) => error instanceof Error ? error : new Error(String(error))
            );
            const waitFailure = await Promise.race([scheduledWait, captureFailureSignal]);
            if (waitFailure && !firstCaptureFailure) firstCaptureFailure = waitFailure;
            if (firstCaptureFailure) break;
            if (inFlightCaptures.size >= maxInFlightCaptures) {
              await Promise.race(inFlightCaptures);
            }
            if (firstCaptureFailure) break;
            const task = capture(canvas, config.screenshotNames[index]).then(
              () => null,
              (error) => {
                const failure = error instanceof Error ? error : new Error(String(error));
                if (!firstCaptureFailure) {
                  firstCaptureFailure = failure;
                  signalCaptureFailure(failure);
                }
                return failure;
              }
            );
            inFlightCaptures.add(task);
            captureResults.push(task);
            void task.finally(() => inFlightCaptures.delete(task));
          }
          const failures = (await Promise.all(captureResults)).filter(Boolean);
          if (firstCaptureFailure) throw firstCaptureFailure;
          if (failures.length > 0) throw failures[0];
          await postDone(0);
        } catch (error) {
          console.error('[fastled-test] ' + (error && error.stack ? error.stack : String(error)));
          try { await postDone(1); } catch (_) {}
        }
      }));
    };
    tryFind();
  };
  document.addEventListener('DOMContentLoaded', findCanvas, { once: true });
  if (document.readyState !== 'loading') findCanvas();
})();
"#;

fn env_flag_enabled(value: Option<&str>) -> bool {
    matches!(value, Some(v) if !v.is_empty() && v != "0")
}

fn viewer_logs_enabled() -> bool {
    let value = std::env::var("FASTLED_VIEWER_LOGS").ok();
    env_flag_enabled(value.as_deref())
}

pub fn run(options: ViewerOptions) -> ExitCode {
    let ViewerOptions {
        url,
        title,
        width,
        height,
        inject_test_runtime,
    } = options;

    let result = tauri::Builder::default()
        .setup(move |app| {
            let url = tauri::WebviewUrl::External(url.parse()?);

            let mut builder = tauri::WebviewWindowBuilder::new(app, "main", url)
                .title(&title)
                .inner_size(width as f64, height as f64);
            if inject_test_runtime {
                builder = builder.initialization_script(TEST_CAPABILITY_SCRIPT);
            }
            if viewer_logs_enabled() || inject_test_runtime {
                builder = builder.initialization_script(LOG_FORWARD_SCRIPT);
            }
            if inject_test_runtime {
                builder = builder.initialization_script(TEST_RUNTIME_SCRIPT);
            }
            let window = builder.build()?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewer_logs_flag_parsing() {
        assert!(!env_flag_enabled(None));
        assert!(!env_flag_enabled(Some("")));
        assert!(!env_flag_enabled(Some("0")));
        assert!(env_flag_enabled(Some("1")));
        assert!(env_flag_enabled(Some("true")));
    }

    #[test]
    fn log_forward_script_targets_server_endpoint() {
        assert!(LOG_FORWARD_SCRIPT.contains("/viewer-log"));
        assert!(LOG_FORWARD_SCRIPT.contains("Bearer ' + window.__fastled_test_token"));
        assert!(LOG_FORWARD_SCRIPT.contains("unhandledrejection"));
        assert!(LOG_FORWARD_SCRIPT.contains("window.addEventListener('error'"));
    }

    #[test]
    fn test_runtime_patches_webgl_before_capturing() {
        assert!(TEST_RUNTIME_SCRIPT.contains("preserveDrawingBuffer: true"));
        assert!(TEST_RUNTIME_SCRIPT.contains("type: 'start_recording'"));
        assert!(TEST_RUNTIME_SCRIPT.contains("canvas.captureStream(0)"));
        assert!(TEST_RUNTIME_SCRIPT.contains("new ImageCapture(track).grabFrame()"));
        assert!(TEST_RUNTIME_SCRIPT.contains("type === 'frame_update'"));
        assert!(TEST_RUNTIME_SCRIPT.contains("document.getElementById('myCanvas')"));
        assert!(TEST_RUNTIME_SCRIPT.contains("manager.isWorkerActive"));
        assert!(TEST_RUNTIME_SCRIPT.contains("frame:rendered"));
        assert!(TEST_RUNTIME_SCRIPT.contains("requestAnimationFrame(() => requestAnimationFrame"));
        assert!(TEST_RUNTIME_SCRIPT.contains("/test-ready"));
        assert!(TEST_RUNTIME_SCRIPT.contains("/viewer-screenshot?name="));
        assert!(TEST_RUNTIME_SCRIPT.contains("firstCaptureAt + index * config.intervalMs"));
        assert!(TEST_CAPABILITY_SCRIPT.contains("fastled-test-token"));
        assert!(TEST_CAPABILITY_SCRIPT.contains("history.replaceState"));
        assert!(TEST_RUNTIME_SCRIPT.contains("testFetch('/test-sleep?ms='"));
        assert!(TEST_RUNTIME_SCRIPT.contains("'Authorization': 'Bearer '"));
        assert!(TEST_RUNTIME_SCRIPT.contains("maxInFlightCaptures = 2"));
        assert!(TEST_RUNTIME_SCRIPT.contains("Promise.race([scheduledWait, captureFailureSignal])"));
        assert!(TEST_RUNTIME_SCRIPT.contains("await Promise.all(captureResults)"));
        assert!(TEST_RUNTIME_SCRIPT.contains("/test-done"));
    }
}
