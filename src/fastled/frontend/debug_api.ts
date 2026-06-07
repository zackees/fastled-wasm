// @ts-nocheck
/// <reference path="types.d.ts" />

/**
 * Global debugging and control surface for the FastLED controller.
 *
 * These functions are exposed to `window` for external access and debugging.
 * They operate on the shared `state.fastLEDController` instance so they
 * remain functional regardless of which module created the controller.
 */

import { state } from './state.ts';

/**
 * Gets the current FastLED controller instance
 * @returns {Object|null} Current controller instance or null
 */
export function getFastLEDController() {
  return state.fastLEDController;
}

/**
 * Gets performance statistics from the current controller
 * @returns {Object|null} Performance stats or null if no controller
 */
export function getFastLEDPerformanceStats() {
  return state.fastLEDController ? state.fastLEDController.getPerformanceStats() : null;
}

/**
 * Starts the FastLED animation loop (for external control)
 * @returns {Promise<void>} Promise that resolves when start is complete
 */
export async function startFastLED() {
  if (!state.fastLEDController) {
    console.error('FastLED controller not initialized');
    return;
  }
  await state.fastLEDController.startWithWorkerSupport();
}

/**
 * Stops the FastLED animation loop (for external control)
 * @returns {Promise<boolean>} Promise that resolves to true if stopped successfully, false otherwise
 */
export async function stopFastLED() {
  if (!state.fastLEDController) {
    console.error('FastLED controller not initialized');
    return false;
  }
  await state.fastLEDController.stopWithWorkerSupport();
  return true;
}

/**
 * Toggles the FastLED animation loop
 * @returns {Promise<boolean>} Promise that resolves to running state (true if now running)
 */
export async function toggleFastLED() {
  if (!state.fastLEDController) {
    console.error('FastLED controller not initialized');
    return false;
  }

  if (state.fastLEDController.running) {
    await state.fastLEDController.stopWithWorkerSupport();
    return false;
  } else {
    await state.fastLEDController.startWithWorkerSupport();
    return true;
  }
}

/**
 * Sets up global error handlers for unhandled promise rejections
 * This helps catch async errors that might otherwise be silent
 */
export function setupGlobalErrorHandlers() {
  // Handle unhandled promise rejections
  window.addEventListener('unhandledrejection', (event) => {
    console.error('Unhandled promise rejection in FastLED:', event.reason);

    // Check if this is a FastLED-related error
    if (event.reason && (
      event.reason.message?.includes('FastLED')
      || event.reason.message?.includes('extern_setup')
      || event.reason.message?.includes('extern_loop')
      || event.reason.stack?.includes('AsyncFastLEDController')
    )) {
      console.error('FastLED async error detected - stopping animation loop');
      if (state.fastLEDController) {
        state.fastLEDController.stop();
      }

      // Show user-friendly error message
      const errorDisplay = document.getElementById('error-display');
      if (errorDisplay) {
        errorDisplay.textContent = 'FastLED encountered an error. Animation stopped.';
      }
    }
  });

  // Handle general errors
  window.addEventListener('error', (event) => {
    if (event.error && event.error.stack?.includes('AsyncFastLEDController')) {
      console.error('FastLED error detected:', event.error);
      if (state.fastLEDController) {
        state.fastLEDController.stop();
      }
    }
  });
}

/**
 * Installs the debug API on the global `window` object and wires up the
 * global error handlers.
 *
 * The original `index.ts` performed these `window.*` assignments at the
 * bottom of the file as a side effect of module load; this preserves that
 * behavior in a single, explicit call that `index.ts` invokes.
 */
export function installDebugApi() {
  // Expose debugging functions globally
  window.getFastLEDController = getFastLEDController;
  window.getFastLEDPerformanceStats = getFastLEDPerformanceStats;
  window.startFastLED = startFastLED;
  window.stopFastLED = stopFastLED;
  window.toggleFastLED = toggleFastLED;

  // Set up global error handlers
  setupGlobalErrorHandlers();
}
