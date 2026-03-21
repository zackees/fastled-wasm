async function loadThreeJSModules() {
  workerLog("LOG", "BACKGROUND_WORKER", "Loading ThreeJS modules in worker context...");
  try {
    const [
      THREE,
      { EffectComposer },
      { RenderPass },
      { UnrealBloomPass },
      { OutputPass },
      BufferGeometryUtils
    ] = await Promise.all([
      import("./assets/three.module-13xj88y1.js"),
      import("./assets/EffectComposer-DFwmeXDn.js"),
      import("./assets/RenderPass-Dn9_edjb.js"),
      import("./assets/UnrealBloomPass-DOx98EJY.js"),
      import("./assets/OutputPass-BVJ8eGwB.js"),
      import("./assets/BufferGeometryUtils-DurnHo24.js")
    ]);
    const modules = {
      THREE,
      EffectComposer,
      RenderPass,
      UnrealBloomPass,
      OutputPass,
      BufferGeometryUtils
    };
    workerLog("LOG", "BACKGROUND_WORKER", "ThreeJS modules loaded successfully", {
      moduleNames: Object.keys(modules)
    });
    return modules;
  } catch (error) {
    workerLog("ERROR", "BACKGROUND_WORKER", "Failed to load ThreeJS modules", error);
    throw error;
  }
}
const workerState = {
  initialized: false,
  canvas: null,
  fastledModule: null,
  graphicsManager: null,
  running: false,
  frameRate: 60,
  capabilities: null,
  renderingContext: null,
  animationFrameId: null,
  frameCount: 0,
  startTime: performance.now(),
  lastFrameTime: 0,
  averageFrameTime: 16.67,
  // Default to ~60 FPS
  // Function references
  externFunctions: null,
  wasmFunctions: null,
  processUiInput: null,
  // URL parameters passed from main thread
  urlParams: {},
  // Frame capture for main-thread recording (MediaRecorder runs on main thread)
  isCapturingFrames: false,
  frameCaptureInterval: 16.67,
  // Default 60 FPS
  lastFrameCaptureTime: 0,
  // Screenmap caching - updated via push notifications from C++ when screenmaps change
  // C++ calls js_notify_screenmap_update() → handleScreenMapUpdate() → cache update
  // Zero polling overhead - completely event-driven
  // Dictionary format: { "0": {strips: {...}, absMin: [...], absMax: [...]}, "1": {...} }
  screenMaps: {},
  // Audio sample queue - samples buffered here from onmessage, flushed to WASM at frame start
  audioSampleQueue: [],
  audioSampleBufferedOnce: false,
  audioSampleFlushedOnce: false
};
const performanceMonitor = {
  frameTimes: [],
  maxSamples: 60,
  lastStatsReport: 0,
  statsReportInterval: 1e3
  // Report every second
};
function workerLog(level, module, message, data = null) {
  const timestamp = (performance.now() - workerState.startTime).toFixed(1);
  const logData = {
    timestamp: `${timestamp}ms`,
    worker: true,
    level,
    module,
    message,
    ...data && { data }
  };
  console[level.toLowerCase()](
    `[${timestamp}ms] [WORKER] [${level}] [${module}] ${message}`,
    data || ""
  );
  postMessage({
    type: "debug_log",
    payload: logData
  });
}
self.onmessage = async function(event) {
  const { type, id, payload } = event.data;
  workerLog("TRACE", "BACKGROUND_WORKER", "Message received", { type, id });
  if (!type) {
    workerLog("TRACE", "BACKGROUND_WORKER", "Ignoring message without type", event.data);
    return;
  }
  try {
    let response = null;
    switch (type) {
      case "initialize":
        response = await handleInitialize(payload);
        break;
      case "start":
        response = await handleStart(payload);
        break;
      case "stop":
        response = await handleStop(payload);
        break;
      case "ping":
        response = handlePing(payload);
        break;
      case "update_frame_rate":
        response = handleUpdateFrameRate(payload);
        break;
      case "get_performance_stats":
        response = getPerformanceStats();
        break;
      case "ui_changes":
        response = handleUiChanges(payload);
        break;
      case "start_recording":
        response = await handleStartRecording(payload);
        break;
      case "stop_recording":
        response = await handleStopRecording(payload);
        break;
      case "screenmap_update":
        console.log("[WORKER] Received screenmap_update message, payload:", payload);
        response = handleScreenMapUpdate(payload);
        console.log("[WORKER] handleScreenMapUpdate completed, response:", response);
        break;
      case "audio_samples":
        handleAudioSamples(payload);
        break;
      default:
        throw new Error(`Unknown message type: ${type}`);
    }
    if (id && response !== null) {
      console.log("🔧 [WORKER] About to send response:", {
        type: `${type}_response`,
        id,
        hasPayload: !!response,
        success: true
      });
      postMessage({
        type: `${type}_response`,
        id,
        payload: response,
        success: true
      });
      console.log("🔧 [WORKER] Response sent for message:", id);
    } else {
      console.log("🔧 [WORKER] NOT sending response:", {
        hasId: !!id,
        hasResponse: response !== null,
        originalType: type
      });
    }
  } catch (error) {
    workerLog("ERROR", "BACKGROUND_WORKER", "Message handling error", {
      type,
      error: error.message,
      stack: error.stack
    });
    if (id) {
      postMessage({
        type: `${type}_response`,
        id,
        payload: { error: error.message },
        success: false
      });
    }
    postMessage({
      type: "error",
      payload: {
        source: "message_handler",
        message: error.message,
        stack: error.stack,
        messageType: type
      }
    });
  }
};
async function handleInitialize(payload) {
  workerLog("LOG", "BACKGROUND_WORKER", "Initializing worker...");
  try {
    workerState.canvas = payload.canvas;
    workerState.capabilities = payload.capabilities;
    workerState.frameRate = payload.frameRate || 60;
    workerState.urlParams = payload.urlParams || {};
    workerLog("LOG", "BACKGROUND_WORKER", "URL parameters received from main thread", workerState.urlParams);
    if (!workerState.canvas || !(workerState.canvas instanceof OffscreenCanvas)) {
      throw new Error("Invalid OffscreenCanvas provided to worker");
    }
    await initializeFastLEDModule();
    await initializeGraphicsManager();
    workerState.initialized = true;
    const result = {
      success: true,
      capabilities: workerState.capabilities,
      canvas: {
        width: workerState.canvas.width,
        height: workerState.canvas.height
      },
      contextType: "webgl2"
    };
    workerLog("LOG", "BACKGROUND_WORKER", "Worker initialized successfully", result);
    postMessage({
      type: "worker_ready",
      payload: result
    });
    return result;
  } catch (error) {
    workerLog("ERROR", "BACKGROUND_WORKER", "Worker initialization failed", error);
    throw error;
  }
}
async function initializeFastLEDModule() {
  workerLog("LOG", "BACKGROUND_WORKER", "Loading FastLED WASM module...");
  try {
    if (typeof self.fastled !== "function") {
      workerLog("LOG", "BACKGROUND_WORKER", "Dynamically loading fastled.js...");
      const fastledScriptPath = new URL("./fastled.js", self.location.href).href;
      workerLog("LOG", "BACKGROUND_WORKER", `Fetching: ${fastledScriptPath}`);
      const response = await fetch(fastledScriptPath);
      if (!response.ok) {
        throw new Error(`Failed to fetch fastled.js: ${response.status} ${response.statusText}`);
      }
      const scriptText = await response.text();
      workerLog("LOG", "BACKGROUND_WORKER", `Fetched ${scriptText.length} bytes, evaluating...`);
      const scriptFunc = new Function(`${scriptText}
return fastled;`);
      self.fastled = scriptFunc.call(self);
      workerLog("LOG", "BACKGROUND_WORKER", "fastled.js evaluated successfully");
    }
    if (typeof self.fastled !== "function") {
      throw new Error("FastLED module not available after dynamic load");
    }
    workerState.fastledModule = await self.fastled({
      canvas: workerState.canvas,
      // Fix pthread worker creation: Tell Emscripten to use fastled.js for pthread workers,
      // not the current worker script (fastled_background_worker.js)
      mainScriptUrlOrBlob: new URL("./fastled.js", self.location.href).href,
      // Pass URL parameters to WASM module via environment
      // This allows C++ code to access them through getenv() or similar
      preRun: [(Module) => {
        if (workerState.urlParams) {
          for (const [key, value] of Object.entries(workerState.urlParams)) {
            workerLog("LOG", "BACKGROUND_WORKER", `Setting URL param: ${key}=${value}`);
            if (!Module.urlParams) {
              Module.urlParams = {};
            }
            Module.urlParams[key] = value;
          }
        }
      }],
      locateFile: (path) => {
        if (path.endsWith(".wasm")) {
          return `./${path}`;
        }
        return path;
      },
      print: (text) => {
        postMessage({
          type: "stdout",
          payload: { text }
        });
      },
      printErr: (text) => {
        postMessage({
          type: "stderr",
          payload: { text }
        });
      }
    });
    const requiredFunctions = ["_extern_setup", "_extern_loop", "_main"];
    for (const funcName of requiredFunctions) {
      if (typeof workerState.fastledModule[funcName] !== "function") {
        throw new Error(`Required WASM function ${funcName} not found`);
      }
    }
    workerLog("LOG", "BACKGROUND_WORKER", "FastLED WASM module loaded successfully");
  } catch (error) {
    workerLog("ERROR", "BACKGROUND_WORKER", "Failed to load FastLED WASM module", error);
    throw error;
  }
}
async function initializeGraphicsManager() {
  workerLog("LOG", "BACKGROUND_WORKER", "Initializing graphics manager...");
  try {
    const FORCE_FAST_RENDERER = workerState.urlParams["gfx"] === "0";
    const FORCE_THREEJS_RENDERER = workerState.urlParams["gfx"] === "1";
    workerLog("LOG", "BACKGROUND_WORKER", "Graphics mode selection", {
      gfxParam: workerState.urlParams["gfx"],
      FORCE_FAST_RENDERER,
      FORCE_THREEJS_RENDERER
    });
    if (FORCE_FAST_RENDERER) {
      workerLog("LOG", "BACKGROUND_WORKER", "Using Fast GraphicsManager (2D) - forced by URL param gfx=0");
      const graphicsManagerModule = await import("./assets/graphics_manager-CZtJ6dKA.js");
      workerState.graphicsManager = new graphicsManagerModule.GraphicsManager({
        canvas: workerState.canvas,
        // Pass OffscreenCanvas directly (not canvasId)
        usePixelatedRendering: true
      });
    } else {
      const explicitlyRequested = FORCE_THREEJS_RENDERER ? "gfx=1" : "default (gfx=1)";
      workerLog("LOG", "BACKGROUND_WORKER", `ThreeJS renderer (${explicitlyRequested}) - loading ThreeJS modules...`);
      const threeJsModules = await loadThreeJSModules();
      workerLog("LOG", "BACKGROUND_WORKER", "ThreeJS modules loaded, creating GraphicsManagerThreeJS...");
      const graphicsManagerModule = await import("./assets/graphics_manager_threejs-DoLCXVee.js");
      workerState.graphicsManager = new graphicsManagerModule.GraphicsManagerThreeJS({
        canvas: workerState.canvas,
        // Pass OffscreenCanvas directly
        threeJsModules
      });
      workerLog("LOG", "BACKGROUND_WORKER", "Beautiful 3D GraphicsManager (ThreeJS) initialized in worker mode");
    }
    workerLog("LOG", "BACKGROUND_WORKER", "Graphics manager initialized", {
      canvas: {
        width: workerState.canvas.width,
        height: workerState.canvas.height
      }
    });
    if (Object.keys(workerState.screenMaps).length > 0 && workerState.graphicsManager.updateScreenMap) {
      workerState.graphicsManager.updateScreenMap(workerState.screenMaps);
      workerLog("LOG", "BACKGROUND_WORKER", "Pushed cached screenMaps to newly initialized graphics manager");
    }
  } catch (error) {
    workerLog("ERROR", "BACKGROUND_WORKER", "Failed to initialize graphics manager", error);
    throw error;
  }
}
async function handleStart(_payload) {
  workerLog("LOG", "BACKGROUND_WORKER", "Starting animation loop...");
  if (!workerState.initialized) {
    throw new Error("Worker not initialized");
  }
  if (workerState.running) {
    workerLog("LOG", "BACKGROUND_WORKER", "Animation already running");
    return { success: true, already_running: true };
  }
  try {
    if (!workerState.externFunctions) {
      const Module = workerState.fastledModule;
      workerState.externFunctions = {
        externSetup: Module.cwrap("extern_setup", "number", []),
        externLoop: Module.cwrap("extern_loop", "number", [])
      };
    }
    workerState.externFunctions.externSetup();
    workerLog("LOG", "BACKGROUND_WORKER", "FastLED setup completed");
    try {
      const Module = workerState.fastledModule;
      if (!workerState.wasmFunctions) {
        workerState.wasmFunctions = {
          getFrameData: Module.cwrap("getFrameData", "number", ["number"]),
          getScreenMapData: Module.cwrap("getScreenMapData", "number", ["number"]),
          getStripPixelData: Module.cwrap("getStripPixelData", "number", ["number", "number"]),
          freeFrameData: Module.cwrap("freeFrameData", null, ["number"])
        };
      }
      const screenMapSizePtr = Module._malloc(4);
      const screenMapDataPtr = workerState.wasmFunctions.getScreenMapData(screenMapSizePtr);
      if (screenMapDataPtr !== 0) {
        const screenMapSize = Module.getValue(screenMapSizePtr, "i32");
        const screenMapJson = Module.UTF8ToString(screenMapDataPtr, screenMapSize);
        const screenMapData = JSON.parse(screenMapJson);
        workerState.screenMaps = screenMapData;
        if (workerState.graphicsManager && workerState.graphicsManager.updateScreenMap) {
          workerState.graphicsManager.updateScreenMap(screenMapData);
          workerLog("LOG", "BACKGROUND_WORKER", "ScreenMaps fetched and sent to graphics manager", {
            screenMapCount: Object.keys(screenMapData).length
          });
        }
        workerState.wasmFunctions.freeFrameData(screenMapDataPtr);
      } else {
        workerLog("WARN", "BACKGROUND_WORKER", "No screenmap data available after setup");
      }
      Module._free(screenMapSizePtr);
    } catch (error) {
      workerLog("ERROR", "BACKGROUND_WORKER", "Failed to fetch screenmap data", error);
    }
    workerState.running = true;
    workerState.startTime = performance.now();
    workerState.frameCount = 0;
    startAnimationLoop();
    const result = {
      success: true,
      frameRate: workerState.frameRate,
      startTime: workerState.startTime
    };
    postMessage({
      type: "animation_started",
      payload: result
    });
    return result;
  } catch (error) {
    workerLog("ERROR", "BACKGROUND_WORKER", "Failed to start animation", error);
    workerState.running = false;
    throw error;
  }
}
function handleStop(_payload) {
  workerLog("LOG", "BACKGROUND_WORKER", "Stopping animation loop...");
  if (!workerState.running) {
    return { success: true, already_stopped: true };
  }
  workerState.running = false;
  if (workerState.animationFrameId) {
    cancelAnimationFrame(workerState.animationFrameId);
    workerState.animationFrameId = null;
  }
  const result = {
    success: true,
    totalFrames: workerState.frameCount,
    totalTime: performance.now() - workerState.startTime
  };
  postMessage({
    type: "animation_stopped",
    payload: result
  });
  workerLog("LOG", "BACKGROUND_WORKER", "Animation loop stopped", result);
  return result;
}
function handlePing(payload) {
  return {
    type: "pong",
    timestamp: Date.now(),
    workerTime: performance.now(),
    originalTimestamp: payload.timestamp
  };
}
function handleUpdateFrameRate(payload) {
  const oldFrameRate = workerState.frameRate;
  workerState.frameRate = payload.frameRate || 60;
  workerLog("LOG", "BACKGROUND_WORKER", "Frame rate updated", {
    oldFrameRate,
    newFrameRate: workerState.frameRate
  });
  return {
    success: true,
    oldFrameRate,
    newFrameRate: workerState.frameRate
  };
}
function handleUiChanges(payload) {
  workerLog("LOG", "BACKGROUND_WORKER", "Processing UI changes from main thread", payload);
  try {
    const Module = workerState.fastledModule;
    if (!Module || !Module.cwrap) {
      throw new Error("WASM module not available for UI processing");
    }
    if (!workerState.processUiInput) {
      workerState.processUiInput = Module.cwrap("processUiInput", null, ["string"]);
    }
    const jsonString = JSON.stringify(payload.changes);
    workerState.processUiInput(jsonString);
    workerLog("LOG", "BACKGROUND_WORKER", "UI changes processed successfully");
    return {
      success: true,
      changesProcessed: Object.keys(payload.changes || {}).length
    };
  } catch (error) {
    workerLog("ERROR", "BACKGROUND_WORKER", "Failed to process UI changes", error);
    throw error;
  }
}
async function captureAndTransferFrame() {
  if (!workerState.isCapturingFrames || !workerState.canvas) {
    return;
  }
  try {
    const now = performance.now();
    const elapsed = now - workerState.lastFrameCaptureTime;
    if (elapsed < workerState.frameCaptureInterval) {
      return;
    }
    const bitmap = await createImageBitmap(workerState.canvas);
    postMessage(
      {
        type: "frame_update",
        payload: {
          bitmap,
          timestamp: now,
          frameNumber: workerState.frameCount,
          width: workerState.canvas.width,
          height: workerState.canvas.height
        }
      },
      /** @type {*} */
      [bitmap]
    );
    workerState.lastFrameCaptureTime = now;
  } catch (error) {
    workerLog("ERROR", "BACKGROUND_WORKER", "Frame capture failed", error);
  }
}
async function handleStartRecording(payload) {
  workerLog("LOG", "BACKGROUND_WORKER", "Starting frame capture for main-thread recording", payload);
  try {
    if (!workerState.canvas) {
      throw new Error("Canvas not available for frame capture");
    }
    if (workerState.isCapturingFrames) {
      throw new Error("Frame capture already in progress");
    }
    const fps = payload.fps || 60;
    workerState.frameCaptureInterval = 1e3 / fps;
    workerState.isCapturingFrames = true;
    workerState.lastFrameCaptureTime = performance.now();
    workerLog("LOG", "BACKGROUND_WORKER", "Frame capture enabled", {
      fps,
      interval: workerState.frameCaptureInterval
    });
    return {
      success: true,
      fps,
      canvasDimensions: {
        width: workerState.canvas.width,
        height: workerState.canvas.height
      }
    };
  } catch (error) {
    workerLog("ERROR", "BACKGROUND_WORKER", "Failed to start frame capture", error);
    throw error;
  }
}
async function handleStopRecording(_payload) {
  workerLog("LOG", "BACKGROUND_WORKER", "Stopping frame capture");
  try {
    workerState.isCapturingFrames = false;
    workerLog("LOG", "BACKGROUND_WORKER", "Frame capture stopped");
    return {
      success: true
    };
  } catch (error) {
    workerLog("ERROR", "BACKGROUND_WORKER", "Failed to stop frame capture", error);
    throw error;
  }
}
function handleScreenMapUpdate(payload) {
  workerLog("LOG", "BACKGROUND_WORKER", "Screenmap update received", payload);
  try {
    workerState.screenMaps = payload.screenMapData;
    workerLog("LOG", "BACKGROUND_WORKER", "Screenmap cache updated", {
      screenMapCount: Object.keys(workerState.screenMaps || {}).length
    });
    if (workerState.graphicsManager && workerState.graphicsManager.updateScreenMap) {
      workerState.graphicsManager.updateScreenMap(payload.screenMapData);
      workerLog("LOG", "BACKGROUND_WORKER", "Graphics manager notified of screenMap update");
    } else {
      workerLog("WARN", "BACKGROUND_WORKER", "Graphics manager not ready, screenMap will be applied during initialization");
    }
    return {
      success: true,
      cached: true,
      graphicsManagerNotified: !!workerState.graphicsManager
    };
  } catch (error) {
    workerLog("ERROR", "BACKGROUND_WORKER", "Failed to update screenmap cache", error);
    throw error;
  }
}
function handleAudioSamples(payload) {
  workerState.audioSampleQueue.push(payload);
  if (!workerState.audioSampleBufferedOnce) {
    workerState.audioSampleBufferedOnce = true;
    workerLog("LOG", "BACKGROUND_WORKER", "First audio sample buffered (will flush at next frame start)");
  }
}
function flushAudioSamplesToWasm() {
  const Module = workerState.fastledModule;
  if (!Module || !Module.ccall || workerState.audioSampleQueue.length === 0) {
    return;
  }
  const queue = workerState.audioSampleQueue;
  workerState.audioSampleQueue = [];
  for (const payload of queue) {
    try {
      const { samples, count, timestamp } = payload;
      const sampleArray = new Int16Array(samples);
      const byteLength = sampleArray.length * 2;
      const ptr = Module._malloc(byteLength);
      if (!ptr) {
        continue;
      }
      const sampleBytes = new Uint8Array(sampleArray.buffer, sampleArray.byteOffset, byteLength);
      Module.HEAPU8.set(sampleBytes, ptr);
      Module.ccall(
        "pushAudioSamples",
        null,
        ["number", "number", "number"],
        [ptr, count, timestamp]
      );
      Module._free(ptr);
    } catch (error) {
      console.error("Error pushing audio sample to WASM:", error);
    }
  }
  if (!workerState.audioSampleFlushedOnce) {
    workerState.audioSampleFlushedOnce = true;
    workerLog("LOG", "BACKGROUND_WORKER", "First audio sample flushed to WASM");
  }
}
function startAnimationLoop() {
  const frameInterval = 1e3 / workerState.frameRate;
  let lastTime = performance.now();
  function animationFrame(currentTime) {
    if (!workerState.running) {
      return;
    }
    const deltaTime = currentTime - lastTime;
    if (deltaTime >= frameInterval) {
      executeFrameLoop(currentTime);
      lastTime = currentTime - deltaTime % frameInterval;
    }
    workerState.animationFrameId = requestAnimationFrame(animationFrame);
  }
  workerState.animationFrameId = requestAnimationFrame(animationFrame);
  workerLog("LOG", "BACKGROUND_WORKER", "Animation loop started", {
    frameRate: workerState.frameRate,
    frameInterval
  });
}
async function executeFrameLoop(currentTime) {
  const frameStartTime = performance.now();
  try {
    workerState.frameCount++;
    flushAudioSamplesToWasm();
    if (!workerState.externFunctions) {
      const Module = workerState.fastledModule;
      workerState.externFunctions = {
        externSetup: Module.cwrap("extern_setup", "number", []),
        externLoop: Module.cwrap("extern_loop", "number", [])
      };
    }
    workerState.externFunctions.externLoop();
    const frameData = extractFrameData();
    if (frameData) {
      workerState.graphicsManager.updateCanvas(frameData);
      if (workerState.isCapturingFrames) {
        captureAndTransferFrame().catch((err) => {
          workerLog("ERROR", "BACKGROUND_WORKER", "Frame capture error", err);
        });
      }
      postMessage({
        type: "frame_rendered",
        payload: {
          frameNumber: workerState.frameCount,
          timestamp: currentTime,
          frameTime: performance.now() - frameStartTime
        }
      });
    }
    const frameTime = performance.now() - frameStartTime;
    updatePerformanceMetrics(frameTime);
  } catch (error) {
    workerLog("ERROR", "BACKGROUND_WORKER", "Frame execution error", error);
    postMessage({
      type: "error",
      payload: {
        source: "frame_loop",
        message: error.message,
        frameNumber: workerState.frameCount
      }
    });
  }
}
function extractFrameData() {
  try {
    const Module = workerState.fastledModule;
    if (!Module.cwrap) {
      workerLog("ERROR", "BACKGROUND_WORKER", "Module.cwrap not available");
      return null;
    }
    if (!workerState.wasmFunctions) {
      workerState.wasmFunctions = {
        getFrameData: Module.cwrap("getFrameData", "number", ["number"]),
        getScreenMapData: Module.cwrap("getScreenMapData", "number", ["number"]),
        getStripPixelData: Module.cwrap("getStripPixelData", "number", ["number", "number"]),
        freeFrameData: Module.cwrap("freeFrameData", null, ["number"])
      };
    }
    const funcs = workerState.wasmFunctions;
    const dataSizePtr = Module._malloc(4);
    const frameDataPtr = funcs.getFrameData(dataSizePtr);
    if (!frameDataPtr) {
      Module._free(dataSizePtr);
      return null;
    }
    const dataSize = Module.getValue(dataSizePtr, "i32");
    const frameDataJson = Module.UTF8ToString(frameDataPtr, dataSize);
    const frameData = JSON.parse(frameDataJson);
    for (const stripData of frameData) {
      const pixelSizePtr = Module._malloc(4);
      const pixelDataPtr = funcs.getStripPixelData(stripData.strip_id, pixelSizePtr);
      if (pixelDataPtr) {
        const pixelSize = Module.getValue(pixelSizePtr, "i32");
        const pixelData = new Uint8Array(Module.HEAPU8.buffer, pixelDataPtr, pixelSize);
        stripData.pixel_data = new Uint8Array(pixelData);
      } else {
        stripData.pixel_data = null;
      }
      Module._free(pixelSizePtr);
    }
    funcs.freeFrameData(frameDataPtr);
    Module._free(dataSizePtr);
    return frameData;
  } catch (error) {
    workerLog("ERROR", "BACKGROUND_WORKER", "Failed to extract frame data", error);
    return null;
  }
}
function updatePerformanceMetrics(frameTime) {
  performanceMonitor.frameTimes.push(frameTime);
  if (performanceMonitor.frameTimes.length > performanceMonitor.maxSamples) {
    performanceMonitor.frameTimes.shift();
  }
  const sum = performanceMonitor.frameTimes.reduce((a, b) => a + b, 0);
  workerState.averageFrameTime = sum / performanceMonitor.frameTimes.length;
  const now = performance.now();
  if (now - performanceMonitor.lastStatsReport > performanceMonitor.statsReportInterval) {
    reportPerformanceStats();
    performanceMonitor.lastStatsReport = now;
  }
}
function reportPerformanceStats() {
  const stats = getPerformanceStats();
  postMessage({
    type: "performance_stats",
    payload: stats
  });
}
function getPerformanceStats() {
  const currentTime = performance.now();
  const runTime = currentTime - workerState.startTime;
  const fps = workerState.frameCount / (runTime / 1e3);
  return {
    fps: fps || 0,
    averageFrameTime: workerState.averageFrameTime,
    frameCount: workerState.frameCount,
    runTime,
    memoryUsage: "self" in globalThis && "performance" in self && self.performance.memory ? {
      usedJSHeapSize: self.performance.memory.usedJSHeapSize,
      totalJSHeapSize: self.performance.memory.totalJSHeapSize,
      jsHeapSizeLimit: self.performance.memory.jsHeapSizeLimit
    } : null,
    isRunning: workerState.running,
    initialized: workerState.initialized
  };
}
self.onerror = function(error) {
  const errorData = typeof error === "string" ? { message: error, filename: "", lineno: 0, colno: 0 } : {
    message: error.message,
    filename: error.filename || "",
    lineno: error.lineno || 0,
    colno: error.colno || 0
  };
  workerLog("ERROR", "BACKGROUND_WORKER", "Worker error", errorData);
  postMessage({
    type: "error",
    payload: {
      source: "worker_error",
      message: errorData.message,
      filename: errorData.filename,
      lineno: errorData.lineno
    }
  });
};
self.onunhandledrejection = function(event) {
  workerLog("ERROR", "BACKGROUND_WORKER", "Unhandled promise rejection", {
    reason: event.reason
  });
  postMessage({
    type: "error",
    payload: {
      source: "promise_rejection",
      message: event.reason?.message || "Unhandled promise rejection",
      reason: event.reason
    }
  });
};
workerLog("LOG", "BACKGROUND_WORKER", "Background worker script loaded and ready");
postMessage({
  type: "worker_script_loaded",
  payload: {
    timestamp: Date.now(),
    capabilities: {
      offscreenCanvas: typeof OffscreenCanvas !== "undefined",
      webgl2: true,
      // Will be verified during initialization
      sharedArrayBuffer: typeof SharedArrayBuffer !== "undefined"
    }
  }
});
//# sourceMappingURL=fastled_background_worker.js.map
