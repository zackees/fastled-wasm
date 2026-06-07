// @ts-nocheck
/// <reference path="types.d.ts" />

/**
 * Canvas rendering bridge for the FastLED frontend.
 *
 * Owns the `updateCanvas(frameData)` entry point that the C++ side calls
 * (indirectly via `window.updateCanvas`) once per frame. Lazily constructs
 * either the Fast 2D or beautiful ThreeJS graphics manager based on the
 * gfx URL parameter, and forwards frame data to it.
 *
 * Shared mutable state (graphicsManager, graphicsArgs, canvasId, etc.)
 * lives in `./state.ts` so other modules can read it.
 */

import { GraphicsManager } from './modules/graphics/graphics_manager.ts';
import { GraphicsManagerThreeJS } from './modules/graphics/graphics_manager_threejs.ts';
import { FORCE_FAST_RENDERER, FORCE_THREEJS_RENDERER } from './bootstrap.ts';
import { state } from './state.ts';

/**
 * @typedef {Object} FrameData
 * @property {number} strip_id - ID of the LED strip
 * @property {string} type - Type of frame data
 * @property {Uint8Array|number[]} pixel_data - Pixel color data
 * @property {ScreenMapData} screenMap - Screen mapping data for LED positions
 */

/**
 * @typedef {Object} ScreenMapData
 * @property {number[]} [absMax] - Maximum coordinates array (computed on-demand)
 * @property {number[]} [absMin] - Minimum coordinates array (computed on-demand)
 * @property {{ [key: string]: any }} strips - Strip configuration data
 */

/**
 * Updates the canvas with new frame data from FastLED
 * @param {FrameData | (Array & {screenMap?: ScreenMapData})} frameData - Frame data with pixel information and screen mapping
 */
export function updateCanvas(frameData) {
  // we are going to add the screenMap to the graphicsManager
  if (frameData.screenMap === undefined) {
    console.warn('Screen map not found in frame data, skipping canvas update');
    return;
  }
  if (!state.graphicsManager) {
    // Ensure graphicsArgs has required properties
    const currentGraphicsArgs = {
      canvasId: state.canvasId || 'canvas',
      threeJsModules: state.graphicsArgs.threeJsModules || null,
      ...state.graphicsArgs
    };

    // Try to create graphics manager - default to ThreeJS (gfx=1) if not specified
    try {
      if (FORCE_FAST_RENDERER) {
        console.log('Creating Fast GraphicsManager with canvas ID (gfx=0)', state.canvasId);
        state.graphicsManager = new GraphicsManager(currentGraphicsArgs);
      } else {
        // Default to ThreeJS renderer (gfx=1) if no parameter specified
        const explicitlyRequested = FORCE_THREEJS_RENDERER ? 'gfx=1' : 'default (gfx=1)';
        console.log(`Creating Beautiful GraphicsManager with canvas ID (${explicitlyRequested})`, state.canvasId);
        state.graphicsManager = new GraphicsManagerThreeJS(currentGraphicsArgs);
      }
    } catch (error) {
      console.error('Failed to create graphics manager:', error);

      const errorDisplay = document.getElementById('error-display');
      if (errorDisplay) {
        errorDisplay.textContent = `Graphics initialization failed: ${error.message}`;
        errorDisplay.style.color = '#ff6b6b'; // Red error color
      }
      return; // Exit early on failure
    }

    // Expose graphics manager globally for video recorder initialization
    window.graphicsManager = state.graphicsManager;

    state.uiCanvasChanged = false;
  }

  if (state.uiCanvasChanged) {
    state.uiCanvasChanged = false;
    try {
      state.graphicsManager.reset();
    } catch (resetError) {
      console.error('Graphics manager reset failed:', resetError);
      // Try to recreate the graphics manager
      state.graphicsManager = null;
      updateCanvas(frameData); // Recursive call to recreate
      return;
    }
  }

  try {
    state.graphicsManager.updateCanvas(frameData);
  } catch (updateError) {
    console.error('Graphics manager update failed:', updateError);

    // Show error to user
    const errorDisplay = document.getElementById('error-display');
    if (errorDisplay) {
      errorDisplay.textContent = 'Rendering error occurred';
      errorDisplay.style.color = '#ff6b6b';
    }
  }
}

// Expose rendering function globally (main thread only)
window.updateCanvas = updateCanvas;
