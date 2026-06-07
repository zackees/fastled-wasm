// @ts-nocheck
/// <reference path="types.d.ts" />

/**
 * Bootstrap module for the FastLED frontend.
 *
 * Holds URL parameter parsing, browser compatibility detection, and
 * forced-renderer flag derivation. Importing this module is side-effect free
 * for module-load purposes; callers explicitly invoke `checkBrowserCompatibility`
 * to perform the compatibility gate (so we preserve the original ordering
 * where the IIFE ran before the rest of `index.ts` set up state).
 */

import { fastLEDWorkerManager } from './modules/core/fastled_worker_manager.ts';

/** URL parameters for runtime configuration */
export const urlParams = new URLSearchParams(window.location.search);

/** Force fast 2D renderer when gfx=0 URL parameter is present */
export const FORCE_FAST_RENDERER = urlParams.get('gfx') === '0';

/** Force beautiful 3D renderer when gfx=1 URL parameter is present */
export const FORCE_THREEJS_RENDERER = urlParams.get('gfx') === '1';

/**
 * Browser Compatibility Check
 * FastLED WASM requires OffscreenCanvas and WebGL2 support for Web Worker mode.
 *
 * Must be invoked once, very early, before any other initialization that
 * depends on those APIs being present.
 */
export function checkBrowserCompatibility() {
  const errors = [];
  const workerCapabilities = fastLEDWorkerManager.capabilities;
  const skipWebGL2Compatibility = urlParams.get('runtime_stack_smoke') === '1';

  // Check OffscreenCanvas support
  if (workerCapabilities?.offscreenCanvas === true) {
    // Reuse the worker manager probe when it has already run during module import.
  } else if (typeof OffscreenCanvas === 'undefined') {
    errors.push('OffscreenCanvas not supported');
  } else {
    // Check WebGL2 support with OffscreenCanvas
    try {
      const testCanvas = new OffscreenCanvas(1, 1);
      const ctx = testCanvas.getContext('webgl2');
      if (!ctx && !skipWebGL2Compatibility && workerCapabilities?.webgl2 !== true) {
        errors.push('WebGL2 not supported with OffscreenCanvas');
      }
    } catch (error) {
      if (!skipWebGL2Compatibility && workerCapabilities?.webgl2 !== true) {
        errors.push(`OffscreenCanvas WebGL2 test failed: ${error.message}`);
      }
    }
  }

  if (!skipWebGL2Compatibility && workerCapabilities && workerCapabilities.webgl2 !== true) {
    errors.push('WebGL2 not supported with OffscreenCanvas');
  }

  if (errors.length > 0) {
    const errorMessage = `
      <div style="
        position: fixed;
        top: 50%;
        left: 50%;
        transform: translate(-50%, -50%);
        background: #ff6b6b;
        color: white;
        padding: 30px;
        border-radius: 10px;
        font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
        max-width: 600px;
        box-shadow: 0 4px 20px rgba(0,0,0,0.3);
        z-index: 10000;
      ">
        <h2 style="margin-top: 0;">Browser Not Supported</h2>
        <p>This browser doesn't support the required features for FastLED WASM:</p>
        <ul style="text-align: left;">
          ${errors.map(err => `<li>${err}</li>`).join('')}
        </ul>
        <p><strong>Please use one of these browsers:</strong></p>
        <ul style="text-align: left;">
          <li>Chrome 69+ or Edge 79+</li>
          <li>Firefox 105+</li>
          <li>Safari 16.4+</li>
        </ul>
      </div>
    `;

    document.addEventListener('DOMContentLoaded', () => {
      document.body.insertAdjacentHTML('beforeend', errorMessage);
    });

    // Throw error to prevent further loading
    throw new Error(`Browser compatibility check failed: ${errors.join(', ')}`);
  }

  console.log('✅ Browser compatibility check passed');
}

/** Logs the resolved gfx parameter and forced-renderer flag values. */
export function logGfxFlags() {
  console.log('🔍 GFX Parameter Debug:');
  console.log('  URL gfx param value:', urlParams.get('gfx'));
  console.log('  FORCE_FAST_RENDERER:', FORCE_FAST_RENDERER);
  console.log('  FORCE_THREEJS_RENDERER:', FORCE_THREEJS_RENDERER);
}
