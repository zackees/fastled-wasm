// @ts-nocheck
/**
 * FastLED UI Manager Debug API
 *
 * Installs the global window.* helpers that expose JsonUiManager debug and
 * spillover-configuration controls. Extracted from `ui_manager.ts` to keep
 * the manager class file focused.
 *
 * Exposes (on first call):
 * - `window.setUiDebug(enabled)`
 * - `window.setUiSpilloverThresholds(config)`
 * - `window.getUiSpilloverThresholds()`
 * - `window.setUiSpilloverExample()`
 *
 * Behavior is preserved exactly as it was inside `ui_manager.ts`.
 *
 * @module UIDebugApi
 */

/* eslint-disable no-console */
/* eslint-disable no-restricted-syntax */
/* eslint-disable max-len */

/**
 * Install the window.* debug helpers used by the UI Manager.
 *
 * Safe to call multiple times — `window` is the install target and each property
 * is overwritten unconditionally (matching the pre-refactor module-load behavior).
 */
export function installUiDebugApi() {
  // Global debug and configuration controls for UI Manager
  if (typeof window !== 'undefined') {
    window.setUiDebug = function (enabled = true) {
      // Access the global UI manager instance if available
      if (window.uiManager && typeof window.uiManager.setDebugMode === 'function') {
        window.uiManager.setDebugMode(enabled);
      } else {
        console.warn(
          '🎵 UI Manager instance not found. Debug mode will be applied when manager is created.',
        );
        // Store the preference for when the manager is created
        window._pendingUiDebugMode = enabled;
      }
    };

    window.setUiSpilloverThresholds = function (config) {
      if (window.uiManager && typeof window.uiManager.updateSpilloverConfig === 'function') {
        window.uiManager.updateSpilloverConfig(config);
      } else {
        console.warn(
          '🎵 UI Manager instance not found. Spillover config will be applied when manager is created.',
        );
        window._pendingSpilloverConfig = config;
      }
    };

    window.getUiSpilloverThresholds = function () {
      if (window.uiManager && typeof window.uiManager.getSpilloverConfig === 'function') {
        return window.uiManager.getSpilloverConfig();
      }
      console.warn('🎵 UI Manager instance not found.');
      return null;
    };

    // Example usage helper
    window.setUiSpilloverExample = function () {
      console.log('🎵 Example spillover configuration:');
      console.log('setUiSpilloverThresholds({');
      console.log('  twoContainer: { minGroups: 3, minElements: 6, minElementsPerGroup: 2 },');
      console.log('  threeContainer: { minGroups: 5, minElements: 10, minElementsPerGroup: 2 }');
      console.log('});');
    };
  }
}
