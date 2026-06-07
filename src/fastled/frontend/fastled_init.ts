// @ts-nocheck
/// <reference path="types.d.ts" />

/**
 * FastLED initialization module.
 *
 * Owns the WASM module loading lifecycle, asyncify-enabled setup/loop,
 * file streaming into the virtual filesystem, and the public
 * `localLoadFastLed(options)` implementation that `loadFastLED` in
 * `index.ts` delegates to.
 */

import { JsonUiManager } from './modules/ui/ui_manager.ts';
import { JsonInspector } from './modules/utils/json_inspector.ts';
import { FastLEDAsyncController } from './modules/core/fastled_async_controller.ts';
import { fastLEDEvents, fastLEDPerformanceMonitor } from './modules/core/fastled_events.ts';
import {
  FASTLED_DEBUG_LOG,
  FASTLED_DEBUG_ERROR,
  FASTLED_DEBUG_TRACE,
} from './modules/core/fastled_debug_logger.ts';

import { state, DEFAULT_FRAME_RATE_60FPS } from './state.ts';
import {
  jsAppendFileUint8,
  partition,
  getFileManifestJson,
} from './vfs_bridge.ts';
import { customPrintFunction } from './logging_setup.ts';
import { toggleFastLED } from './debug_api.ts';

/**
 * Main setup and loop execution function for FastLED programs (Pure JavaScript Architecture)
 * @async
 * @param {Object} moduleInstance - The WASM module instance
 * @param {number} frame_rate - Target frame rate for the animation loop
 * @returns {Promise<void>} Promise that resolves when setup is complete and loop is started
 */
export async function FastLED_SetupAndLoop(moduleInstance, frame_rate) {
  FASTLED_DEBUG_TRACE('INDEX_JS', 'FastLED_SetupAndLoop', 'ENTER', { frame_rate });

  try {
    FASTLED_DEBUG_LOG('INDEX_JS', 'Initializing FastLED with Pure JavaScript Architecture...');
    console.log('Initializing FastLED with Pure JavaScript Architecture...');

    // Check if moduleInstance is valid
    FASTLED_DEBUG_LOG('INDEX_JS', 'Checking moduleInstance', {
      hasModule: !!moduleInstance,
      hasExternSetup: !!(moduleInstance && moduleInstance._extern_setup),
      hasExternLoop: !!(moduleInstance && moduleInstance._extern_loop),
      hasCwrap: !!(moduleInstance && moduleInstance.cwrap),
    });

    if (!moduleInstance) {
      throw new Error('moduleInstance is null or undefined');
    }

    // Create the pure JavaScript async controller
    FASTLED_DEBUG_LOG('INDEX_JS', 'Creating FastLEDAsyncController...');
    state.fastLEDController = new FastLEDAsyncController(moduleInstance, frame_rate);
    FASTLED_DEBUG_LOG('INDEX_JS', 'FastLEDAsyncController created successfully');

    // Expose controller globally for debugging and external control
    window.fastLEDController = state.fastLEDController;

    // Expose event system globally
    window.fastLEDEvents = fastLEDEvents;
    window.fastLEDPerformanceMonitor = fastLEDPerformanceMonitor;

    FASTLED_DEBUG_LOG('INDEX_JS', 'Globals exposed, calling controller.setup()...');

    // Setup FastLED with proper error handling
    console.log('🔧 About to call fastLEDController.setup()');
    try {
      state.fastLEDController.setup();
      console.log('🔧 fastLEDController.setup() completed successfully');
    } catch (error) {
      console.error('🔧 setup() threw error:', error);
      console.error('🔧 Error stack:', error.stack);
      throw error; // Re-throw to prevent silent failure
    }

    FASTLED_DEBUG_LOG('INDEX_JS', 'Controller setup completed, initializing worker mode...');

    // Initialize Web Worker mode for background thread rendering (always enabled)
    const canvas = /** @type {HTMLCanvasElement | null} */ (document.getElementById('myCanvas'));
    if (!canvas) {
      throw new Error('Canvas element not found');
    }

    FASTLED_DEBUG_LOG('INDEX_JS', 'Initializing Web Worker mode with OffscreenCanvas...');
    await state.fastLEDController.initializeWorkerMode(canvas, {
      maxRetries: 3
    });
    FASTLED_DEBUG_LOG('INDEX_JS', 'Web Worker mode initialized successfully');

    // Start the async animation loop in worker mode
    await state.fastLEDController.startWithWorkerSupport();
    FASTLED_DEBUG_LOG('INDEX_JS', 'Animation loop started with Web Worker support');
    console.log('FastLED running in Web Worker mode (background thread)');

    FASTLED_DEBUG_LOG('INDEX_JS', 'Animation loop started, setting up UI controls...');

    // Add UI controls for start/stop if elements exist
    const startBtn = document.getElementById('start-btn');
    const stopBtn = document.getElementById('stop-btn');
    const toggleBtn = document.getElementById('toggle-btn');
    const fpsDisplay = document.getElementById('fps-display');

    FASTLED_DEBUG_LOG('INDEX_JS', 'UI controls found', {
      startBtn: !!startBtn,
      stopBtn: !!stopBtn,
      toggleBtn: !!toggleBtn,
      fpsDisplay: !!fpsDisplay,
    });

    if (startBtn) {
      startBtn.onclick = async () => {
        FASTLED_DEBUG_LOG('INDEX_JS', 'Start button clicked');
        if (state.fastLEDController.setupCompleted) {
          await state.fastLEDController.startWithWorkerSupport();
        } else {
          console.warn('FastLED setup not completed yet');
          FASTLED_DEBUG_LOG('INDEX_JS', 'Start button clicked but setup not completed');
        }
      };
    }

    if (stopBtn) {
      stopBtn.onclick = async () => {
        FASTLED_DEBUG_LOG('INDEX_JS', 'Stop button clicked');
        await state.fastLEDController.stopWithWorkerSupport();
      };
    }

    if (toggleBtn) {
      toggleBtn.onclick = async () => {
        FASTLED_DEBUG_LOG('INDEX_JS', 'Toggle button clicked');
        const isRunning = await toggleFastLED();
        toggleBtn.textContent = isRunning ? 'Pause' : 'Resume';
      };
    }

    // Performance monitoring display with event system integration
    if (fpsDisplay) {
      FASTLED_DEBUG_LOG('INDEX_JS', 'Setting up performance monitoring...');
      setInterval(() => {
        const fps = state.fastLEDController.getFPS();
        const frameTime = state.fastLEDController.getAverageFrameTime();

        // Record performance metrics
        fastLEDPerformanceMonitor.recordFrameTime(frameTime);

        // Update display
        fpsDisplay.textContent = `FPS: ${fps.toFixed(1)} | Frame: ${frameTime.toFixed(1)}ms`;

        // Monitor memory usage if available
        if (performance.memory) {
          fastLEDPerformanceMonitor.recordMemoryUsage(performance.memory.usedJSHeapSize);
        }
      }, 1000);
    }

    // Set up event monitoring for debugging
    if (window.fastLEDDebug) {
      fastLEDEvents.setDebugMode(true);
      FASTLED_DEBUG_LOG('INDEX_JS', 'Event debug mode enabled');
    }

    FASTLED_DEBUG_LOG('INDEX_JS', 'Checking callback function availability...');
    const callbackStatus = {
      FastLED_onFrame: typeof globalThis.FastLED_onFrame,
      FastLED_processUiUpdates: typeof globalThis.FastLED_processUiUpdates,
      FastLED_onStripUpdate: typeof globalThis.FastLED_onStripUpdate,
      FastLED_onStripAdded: typeof globalThis.FastLED_onStripAdded,
      FastLED_onUiElementsAdded: typeof globalThis.FastLED_onUiElementsAdded,
    };

    FASTLED_DEBUG_LOG('INDEX_JS', 'Callback function status', callbackStatus);

    console.log('FastLED Pure JavaScript Architecture initialized successfully');
    console.log('Available features:', {
      asyncController: !!state.fastLEDController,
      eventSystem: !!fastLEDEvents,
      performanceMonitor: !!fastLEDPerformanceMonitor,
      callbacks: callbackStatus,
    });

    FASTLED_DEBUG_LOG('INDEX_JS', 'FastLED_SetupAndLoop completed successfully');
    FASTLED_DEBUG_TRACE('INDEX_JS', 'FastLED_SetupAndLoop', 'EXIT');
  } catch (error) {
    FASTLED_DEBUG_ERROR('INDEX_JS', 'Failed to initialize FastLED with Pure JavaScript Architecture', error);
    console.error('Failed to initialize FastLED with Pure JavaScript Architecture:', error);

    // Emit error event
    if (fastLEDEvents) {
      fastLEDEvents.emitError('initialization', error.message, { stack: error.stack });
    }

    // Show user-friendly error message if error display element exists
    const errorDisplay = document.getElementById('error-display');
    if (errorDisplay) {
      errorDisplay.textContent = 'Failed to load FastLED with Pure JavaScript Architecture. Please refresh the page.';
    }

    throw error;
  }
}

/**
 * Main function to initialize and start the FastLED setup/loop cycle (Asyncify-enabled)
 * @async
 * @param {number} frame_rate - Target frame rate for animations
 * @param {Object} moduleInstance - The loaded WASM module instance
 * @param {Array<Object>} filesJson - Array of files to load into the virtual filesystem
 */
export async function fastledLoadSetupLoop(
  frame_rate,
  moduleInstance,
  filesJson,
) {
  console.log('Calling setup function...');

  const fileManifest = getFileManifestJson(filesJson, frame_rate);
  moduleInstance.cwrap('fastled_declare_files', null, ['string'])(JSON.stringify(fileManifest));
  console.log('Files JSON:', filesJson);

  /**
   * Processes a single file by streaming it to the WASM module
   * @async
   * @param {Object} file - File object with path and data
   * @param {string} file.path - File path in the virtual filesystem
   * @param {number} file.size - File size in bytes
   */
  const processFile = async (file) => {
    try {
      const response = await fetch(file.path);
      const reader = response.body.getReader();

      console.log(`File fetched: ${file.path}, size: ${file.size}`);

      while (true) {
        // deno-lint-ignore no-await-in-loop
        const { value, done } = await reader.read();
        if (done) break;
        // Allocate and copy chunk data
        jsAppendFileUint8(moduleInstance, file.path, value);
      }
    } catch (error) {
      console.error(`Error processing file ${file.path}:`, error);
    }
  };

  /**
   * Fetches all files in parallel and calls completion callback
   * @async
   * @param {Array<Object>} filesJson - Array of file objects to fetch
   * @param {Function} [onComplete] - Optional callback when all files are loaded
   */
  const fetchAllFiles = async (filesJson, onComplete) => {
    const promises = filesJson.map(async (file) => {
      await processFile(file);
    });
    await Promise.all(promises);
    if (onComplete) {
      onComplete();
    }
  };

  // NOTE: Callback functions are now automatically registered by importing fastled_callbacks.js
  // No need to manually bind them here - they're pure JavaScript functions

  // Verify that the pure JavaScript callbacks are properly loaded
  console.log('FastLED Pure JavaScript callbacks verified:', {
    FastLED_onUiElementsAdded: typeof globalThis.FastLED_onUiElementsAdded,
    FastLED_onFrame: typeof globalThis.FastLED_onFrame,
    FastLED_onStripAdded: typeof globalThis.FastLED_onStripAdded,
    FastLED_onStripUpdate: typeof globalThis.FastLED_onStripUpdate,
    FastLED_processUiUpdates: typeof globalThis.FastLED_processUiUpdates,
    FastLED_onError: typeof globalThis.FastLED_onError,
  });

  // Initialize event system integration
  if (fastLEDEvents) {
    console.log('FastLED Event System ready with stats:', fastLEDEvents.getEventStats());
  }

  // Come back to this later - we want to partition the files into immediate and streaming files
  // so that large projects don't try to download ALL the large files BEFORE setup/loop is called.
  const [immediateFiles, streamingFiles] = partition(filesJson, ['.json', '.csv', '.txt', '.cfg']);
  console.log(
    'The following files will be immediatly available and can be read during setup():',
    immediateFiles,
  );
  console.log('The following files will be streamed in during loop():', streamingFiles);

  const promiseImmediateFiles = fetchAllFiles(immediateFiles, () => {
    if (immediateFiles.length !== 0) {
      console.log('All immediate files downloaded to FastLED.');
    }
  });
  await promiseImmediateFiles;
  if (streamingFiles.length > 0) {
    const streamingFilesPromise = fetchAllFiles(streamingFiles, () => {
      console.log('All streaming files downloaded to FastLED.');
    });
    const delay = new Promise((r) => { setTimeout(r, 250); });
    // Wait for either the time delay or the streaming files to be processed, whichever
    // happens first.
    await Promise.any([delay, streamingFilesPromise]);
  }

  console.log('Starting fastled with Asyncify support');
  await FastLED_SetupAndLoop(moduleInstance, frame_rate);
}

/**
 * Callback function executed when the WASM module is loaded
 * Sets up the module loading infrastructure
 * @param {Function} fastLedLoader - The FastLED loader function
 */
export function onModuleLoaded(fastLedLoader) {
  // Unpack the module functions and send them to the fastledLoadSetupLoop function

  /**
   * Internal function to start FastLED with loaded module (Asyncify-enabled)
   * @async
   * @param {Object} moduleInstance - The loaded WASM module instance
   * @param {number} frameRate - Target frame rate for animations
   * @param {Array<Object>} filesJson - Files to load into virtual filesystem
   */
  async function __fastledLoadSetupLoop(moduleInstance, frameRate, filesJson) {
    const exports_exist = moduleInstance && moduleInstance._extern_setup
      && moduleInstance._extern_loop;
    if (!exports_exist) {
      console.error('FastLED setup or loop functions are not available.');
      return;
    }

    await fastledLoadSetupLoop(
      frameRate,
      moduleInstance,
      filesJson,
    );
  }
  // Start fetch now in parallel

  /**
   * Fetches and parses JSON from a given file path
   * @async
   * @param {string} fetchFilePath - Path to the JSON file to fetch
   * @returns {Promise<Object>} Parsed JSON data
   */
  const fetchJson = async (fetchFilePath) => {
    const response = await fetch(fetchFilePath);
    const data = await response.json();
    return data;
  };
  const filesJsonPromise = fetchJson('files.json');
  try {
    if (typeof fastLedLoader === 'function') {
      // Load the module
      fastLedLoader().then(async (instance) => {
        console.log('Module loaded, running FastLED...');

        // Expose the updateUiComponents method to the C++ module
        // This should be called BY C++ TO UPDATE the frontend, not the other way around
        instance._jsUiManager_updateUiComponents = function (jsonString) {
          console.log('*** C++ CALLING JS: updateUiComponents with:', jsonString);
          if (window.uiManagerInstance && window.uiManagerInstance.updateUiComponents) {
            window.uiManagerInstance.updateUiComponents(jsonString);
          } else {
            console.error('*** UI BINDING ERROR: uiManagerInstance not available ***');
          }
        };

        // Wait for the files.json to load.
        let filesJson = null;
        try {
          filesJson = await filesJsonPromise;
          console.log('Files JSON:', filesJson);
        } catch (error) {
          console.error('Error fetching files.json:', error);
          filesJson = {};
        }
        await __fastledLoadSetupLoop(instance, state.frameRate, filesJson);
      }).catch((err) => {
        console.error('Error loading fastled as a module:', err);
      });
    } else {
      console.log(
        'Could not detect a valid module loading for FastLED, expected function but got',
        typeof fastLedLoader,
      );
    }
  } catch (error) {
    console.error('Failed to load FastLED:', error);
  }
}

/**
 * Main FastLED loading and initialization function
 * Sets up the entire FastLED environment including UI, graphics, and WASM module
 * @async
 * @param {Object} options - Configuration options for FastLED initialization
 * @param {string} options.canvasId - ID of the HTML canvas element for rendering
 * @param {string} options.uiControlsId - ID of the HTML element for UI controls
 * @param {string} options.printId - ID of the HTML element for console output
 * @param {number} [options.frameRate] - Target frame rate (defaults to 60 FPS)
 * @param {Object} options.threeJs - Three.js configuration object
 * @param {Object} options.threeJs.modules - Three.js module imports
 * @param {string} options.threeJs.containerId - Container ID for Three.js rendering
 * @param {Function} options.fastled - FastLED WASM module loader function
 * @returns {Promise<void>} Promise that resolves when FastLED is fully loaded
 */
export async function localLoadFastLed(options) {
  try {
    console.log('Loading FastLED with options:', options);
    state.canvasId = options.canvasId;
    state.uiControlsId = options.uiControlsId;
    state.outputId = options.printId;
    state.print = customPrintFunction;
    console.log('Loading FastLED with options:', options);
    state.frameRate = options.frameRate || DEFAULT_FRAME_RATE_60FPS;
    state.uiManager = new JsonUiManager(state.uiControlsId);

    // Initialize JSON Inspector
    new JsonInspector();

    // Expose UI manager globally for debug functions (main thread only)
    window.uiManager = state.uiManager;
    window.uiManagerInstance = state.uiManager;

    // Apply pending debug mode setting if it was set before manager creation
    if (typeof window._pendingUiDebugMode !== 'undefined') {
      state.uiManager.setDebugMode(window._pendingUiDebugMode);
      delete window._pendingUiDebugMode;
    }

    // Set up periodic cleanup of orphaned UI elements
    setInterval(() => {
      if (state.uiManager && state.uiManager.cleanupOrphanedElements) {
        state.uiManager.cleanupOrphanedElements();
      }
    }, 5000); // Run cleanup every 5 seconds

    // Set up periodic UI polling for worker mode
    // In worker mode, there's no main thread loop, so we poll for UI changes
    setInterval(() => {
      if (window.fastLEDWorkerManager && window.fastLEDWorkerManager.isWorkerActive) {
        // Worker mode: poll UI changes and send to worker
        if (window.uiManager && typeof window.uiManager.processUiChanges === 'function') {
          const changes = window.uiManager.processUiChanges();
          if (changes && Object.keys(changes).length > 0) {
            //console.log('🎮 [UI_POLL] Detected UI changes in worker mode:', changes);
            const message = {
              type: 'ui_changes',
              payload: {
                changes: changes
              }
            };
            window.fastLEDWorkerManager.worker.postMessage(message);
            //console.log('🎮 [UI_POLL] UI changes sent to worker');
          }
        }
      }
    }, 1000 / 60); // Poll at 60Hz to match typical frame rate

    const { threeJs } = options;
    console.log('ThreeJS:', threeJs);
    const fastLedLoader = options.fastled;
    state.threeJsModules = threeJs.modules;
    state.containerId = threeJs.containerId;
    console.log('ThreeJS modules:', state.threeJsModules);
    console.log('Container ID:', state.containerId);
    state.graphicsArgs = {
      canvasId: state.canvasId,
      threeJsModules: state.threeJsModules,
    };
    await onModuleLoaded(fastLedLoader);
  } catch (error) {
    console.error('Error loading FastLED:', error);
    // Debug point removed for linting compliance
  }
}
