// @ts-nocheck
/// <reference path="types.d.ts" />

/**
 * FastLED WebAssembly Compiler Main Module
 *
 * This module serves as the main entry point for the FastLED WebAssembly
 * compiler. After the split-into-modules refactor it does very little of its
 * own: it imports sibling modules in the precise order needed to preserve
 * the original initialization sequence and re-exports the public
 * `loadFastLED(options)` API consumed by `app.ts`.
 *
 * Side-effect ordering preserved:
 *   1. Log the loading banner.
 *   2. Run the browser compatibility IIFE (now an explicit call).
 *   3. Log gfx flags.
 *   4. Install the timestamped console override BEFORE any further work so
 *      downstream module logs flow through it.
 *   5. Install the debug API and global error handlers.
 *   6. Importing `canvas_render.ts` assigns `window.updateCanvas` as a side
 *      effect of module load.
 *   7. Wire up video-recording auto-init (worker event + timeout fallback).
 *
 * @module FastLED/Compiler
 */

// Recording System imports - kept here so the side-effect-only modules
// (`ui_recorder`, `ui_playback`, `ui_recorder_test`) run their global
// registrations on module load, exactly as before.
import './modules/recording/ui_recorder.ts';
import './modules/recording/ui_playback.ts';
import './modules/recording/ui_recorder_test.ts';

// Side-effect import: registers FastLED_onFrame, FastLED_onStripUpdate, etc.
// onto globalThis. Must happen before FastLED_SetupAndLoop verifies them.
import './modules/core/fastled_callbacks.ts';

import { checkBrowserCompatibility, urlParams, logGfxFlags } from './bootstrap.ts';
import { installConsoleOverride } from './logging_setup.ts';
import { installDebugApi } from './debug_api.ts';
// Importing canvas_render performs the `window.updateCanvas = updateCanvas`
// assignment as a side effect of module load.
import './canvas_render.ts';
import { localLoadFastLed } from './fastled_init.ts';
import { initVideoRecording } from './video_recording_init.ts';

console.log(`⭐ index.js loading, URL: ${window.location.href}`);

/**
 * Browser Compatibility Check
 * FastLED WASM requires OffscreenCanvas and WebGL2 support for Web Worker mode.
 * This must execute synchronously at module load before anything that depends
 * on those APIs runs.
 */
checkBrowserCompatibility();

logGfxFlags();

/**
 * Stub FastLED loader function (replaced during initialization)
 * @param {Object} _options - Loading options (ignored in stub)
 * @returns {Promise<void>} Promise that resolves when initialization completes
 */
let _loadFastLED = function (_options) {
  // Stub to let the user/dev know that something went wrong.
  // This function is replaced with an async implementation, so it must be async for interface compatibility
  console.log('FastLED loader function was not set.');
  return Promise.resolve();
};

/**
 * Public FastLED loading function (delegates to private implementation)
 * @async
 * @param {Object} options - Loading options and configuration
 * @returns {Promise<*>} Result from the FastLED loader
 */
export async function loadFastLED(options) {
  // This will be overridden through the initialization.
  return await _loadFastLED(options);
}

// Install the timestamped console override AFTER the initial loading banner
// but BEFORE the rest of the init work. Matches original ordering: in the
// monolithic file the override was installed at the same point in the
// top-to-bottom execution sequence (after the IIFE, before subsequent
// module-load side effects from imports executed at the top of the file
// were exercised at runtime via function calls).
installConsoleOverride();

/** Replace the stub loader with the actual implementation */
_loadFastLED = localLoadFastLed;

// NOTE: All callback functions live in fastled_callbacks.ts (imported above).
// Available callbacks:
//   - FastLED_onStripUpdate()       handles strip configuration changes
//   - FastLED_onStripAdded()        handles new strip registration
//   - FastLED_onFrame()             handles frame rendering
//   - FastLED_processUiUpdates()    handles UI state collection
//   - FastLED_onUiElementsAdded()   handles UI element addition
//   - FastLED_onError()             handles error reporting

// Install global debug API (window.getFastLEDController, window.startFastLED, ...)
// and the global unhandled-rejection / error handlers.
installDebugApi();

// Wire up video recorder auto-initialization (worker event + 500ms fallback)
// and expose recorder helpers on window.
initVideoRecording();
