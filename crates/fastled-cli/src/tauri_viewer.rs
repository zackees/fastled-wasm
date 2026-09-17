use std::process::ExitCode;

use kernal_api::async_engine::RuntimeBuilder;
use kernal_api::webview::{
    ExternalWebviewClient, ExternalWebviewHost, WebviewError, WebviewPageBootstrap,
    WebviewPermissions, WebviewWindowOptions,
};

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

/// Forwards `console.*`, uncaught errors, unhandled rejections, and failed
/// fetches to the FastLED HTTP server, which echoes them to stderr.
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
  // Every capture strategy talks to the compositor, the GPU stack or the
  // worker, and any of them can stall instead of failing: on a runner with no
  // GPU, captureStream produces no frames and the worker has no OffscreenCanvas
  // to snapshot. A stalled strategy used to consume the whole
  // --test-timeout-secs with no output (#247), so each one is bounded and
  // announces itself in the viewer log.
  // The page console is the only other channel, and it goes quiet once the
  // harness takes over, so trace progress straight to the server (#247).
  const trace = (message) => {
    try {
      return testFetch('/viewer-log', { method: 'POST', body: '[fastled-test] ' + message });
    } catch (error) {
      return Promise.resolve();
    }
  };
  const CAPTURE_STAGE_TIMEOUT_MS = 15000;
  const captureStage = async (stage, run) => {
    await trace('capture stage ' + stage);
    let timer;
    try {
      const result = await Promise.race([
        run(),
        new Promise((_, reject) => {
          timer = setTimeout(
            () => reject(new Error('capture stage ' + stage + ' timed out')),
            CAPTURE_STAGE_TIMEOUT_MS
          );
        })
      ]);
      await trace('capture stage ' + stage + (result ? ' produced a frame' : ' produced nothing'));
      return result;
    } catch (error) {
      await trace('capture stage ' + stage + ' failed: ' + error);
      return null;
    } finally {
      clearTimeout(timer);
    }
  };
  const capture = async (canvas, name) => {
    // Without WebGL2 on OffscreenCanvas (WebKitGTK) the page canvas draws the
    // frames itself, and preserveDrawingBuffer above keeps them readable.
    const mainThread = !!(window.fastLEDWorkerManager && window.fastLEDWorkerManager.renderOnMainThread);
    await trace('capture ' + name + ' mainThread=' + mainThread);
    let blob = null;
    if (mainThread) {
      const dataUrl = await captureStage('page-canvas', async () => canvas.toDataURL('image/png'));
      if (!dataUrl) throw new Error('the page canvas produced no frame');
      blob = await (await fetch(dataUrl)).blob();
    } else {
      blob = await captureStage('worker-webgl', () => webglFrameBlob());
    }
    if (!blob) {
      const dataUrl = await captureStage('composited', () => compositedFrameDataUrl(canvas))
        || await captureStage('worker-bitmap', () => workerFrameDataUrl())
        || await captureStage('page-canvas', async () => canvas.toDataURL('image/png'));
      if (!dataUrl) throw new Error('every capture strategy failed for ' + name);
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
          await trace('ready acknowledged');
          const response = await testFetch('/test-config');
          if (!response.ok) throw new Error('test config failed: ' + response.status);
          const config = await response.json();
          await trace('config screenshots=' + JSON.stringify(config.screenshotNames)
            + ' waitMs=' + config.waitMs + ' intervalMs=' + config.intervalMs);
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
            await trace('waiting for capture ' + index);
            const waitFailure = await Promise.race([scheduledWait, captureFailureSignal]);
            await trace('wait for capture ' + index + ' finished');
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

// Product policy, not a kernel default. WebKitGTK/WKWebView must not receive
// this Windows-specific compensation. Native scale is not browser zoom/DPR.
const WINDOWS_ZOOM_SCRIPT: &str = r#"
(() => {
  const scale = kernalWindow.initialScaleFactor;
  if (scale > 1) {
    document.addEventListener('DOMContentLoaded', () => {
      document.body.style.zoom = String(0.92 / scale);
    }, { once: true });
  }
})();
"#;

fn bootstrap_source(test_runtime: bool, windows: bool) -> String {
    let mut source = String::new();
    if test_runtime {
        source.push_str(TEST_CAPABILITY_SCRIPT);
    }
    source.push_str(LOG_FORWARD_SCRIPT);
    if test_runtime {
        source.push_str(TEST_RUNTIME_SCRIPT);
    }
    if windows {
        source.push_str(WINDOWS_ZOOM_SCRIPT);
    }
    source
}

struct ViewerExitRequest(ExternalWebviewClient);

impl Drop for ViewerExitRequest {
    fn drop(&mut self) {
        if let Err(error) = self.0.request_exit() {
            eprintln!("fastled: viewer shutdown request failed: {error}");
        }
    }
}

fn run_viewer(options: ViewerOptions) -> Result<(), String> {
    let window = WebviewWindowOptions::new(&options.title, options.width, options.height)
        .map_err(|error| error.to_string())?;
    let bootstrap = WebviewPageBootstrap::new(&bootstrap_source(
        options.inject_test_runtime,
        cfg!(target_os = "windows"),
    ))
    .map_err(|error| error.to_string())?;
    // No runtime worker threads exist while the kernel prepares Linux graphics
    // environment and creates the main-thread host. Drive this one runtime on
    // the lifecycle thread only after host initialization has completed.
    let runtime = RuntimeBuilder::current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    let host = ExternalWebviewHost::new(runtime.handle()).map_err(|error| error.to_string())?;
    let client = host.client();
    let worker = std::thread::Builder::new()
        .name("fastled-viewer-lifecycle".into())
        .spawn(move || {
            let _exit = ViewerExitRequest(client.clone());
            runtime.run(async move {
                let permissions = WebviewPermissions::deny_all().allow_user_media();
                let webview = client
                    .open_webview_with_bootstrap(&options.url, window, permissions, bootstrap)
                    .await?;
                // Interactive windows have no arbitrary lifetime expiry. Do not
                // wait for exact load-URL equality: the test script strips its
                // capability fragment before the load callback can arrive.
                match webview.wait_for_terminal().await {
                    Err(WebviewError::WindowClosed) | Ok(()) => Ok(()),
                    Err(error) => Err(error),
                }
            })
        })
        .map_err(|error| error.to_string())?;
    let host_code = host.run();
    let outcome = worker
        .join()
        .map_err(|_| "viewer lifecycle thread panicked".to_owned())?;
    outcome.map_err(|error| error.to_string())?;
    if host_code != 0 {
        return Err(format!("viewer event loop exited with {host_code}"));
    }
    Ok(())
}

pub fn run(options: ViewerOptions) -> ExitCode {
    match run_viewer(options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("fastled: viewer failed: {err}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_preserves_product_order_and_platform_policy() {
        let test = bootstrap_source(true, false);
        assert!(test.starts_with(TEST_CAPABILITY_SCRIPT));
        assert_eq!(
            test,
            format!("{TEST_CAPABILITY_SCRIPT}{LOG_FORWARD_SCRIPT}{TEST_RUNTIME_SCRIPT}")
        );
        assert_eq!(bootstrap_source(false, false), LOG_FORWARD_SCRIPT);
        let windows = bootstrap_source(true, true);
        assert_eq!(windows, format!("{test}{WINDOWS_ZOOM_SCRIPT}"));
        assert!(WINDOWS_ZOOM_SCRIPT.contains("kernalWindow.initialScaleFactor"));
        assert!(WINDOWS_ZOOM_SCRIPT.contains("0.92 / scale"));
        assert!(!WINDOWS_ZOOM_SCRIPT.contains("devicePixelRatio"));
        assert!(WebviewPageBootstrap::new(&windows).is_ok());
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
        assert!(TEST_RUNTIME_SCRIPT.contains("fastLEDWorkerManager.renderOnMainThread"));
        assert!(TEST_RUNTIME_SCRIPT.contains("canvas.toDataURL('image/png')"));
        assert!(TEST_RUNTIME_SCRIPT.contains("trace('capture stage ' + stage)"));
        assert!(TEST_RUNTIME_SCRIPT.contains("CAPTURE_STAGE_TIMEOUT_MS = 15000"));
        assert!(TEST_RUNTIME_SCRIPT.contains("the page canvas produced no frame"));
        assert!(TEST_RUNTIME_SCRIPT.contains("trace('ready acknowledged')"));
        assert!(TEST_RUNTIME_SCRIPT.contains("testFetch('/viewer-log'"));
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
