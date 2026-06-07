// @ts-nocheck
/// <reference path="types.d.ts" />

/**
 * Shared mutable state used across the FastLED frontend modules.
 *
 * This module holds module-level mutable values that the original
 * `index.ts` kept as top-level `let` bindings. After the split-into-modules
 * refactor, multiple sibling modules need to read/write the same values, so
 * we centralize them here on a single mutable record. Pragmatism over
 * purity: this preserves the exact behavior of the original file without
 * needing to thread state through every function signature.
 */

/** Default frame rate for FastLED animations (60 FPS) */
export const DEFAULT_FRAME_RATE_60FPS = 60;

/** Maximum number of lines to keep in stdout output display */
export const MAX_STDOUT_LINES = 50;

/**
 * @typedef {Object} FastLEDState
 * @property {number} frameRate
 * @property {string|undefined} canvasId
 * @property {string|undefined} uiControlsId
 * @property {string|undefined} outputId
 * @property {any} uiManager
 * @property {boolean} uiCanvasChanged
 * @property {any} threeJsModules
 * @property {any} graphicsManager
 * @property {string|undefined} containerId
 * @property {any} graphicsArgs
 * @property {HTMLCanvasElement|null} mirrorCanvas
 * @property {CanvasRenderingContext2D|null} mirrorCanvasContext
 * @property {any} mainThreadVideoRecorder
 * @property {any} fastLEDController
 * @property {any} videoRecorder
 * @property {Function} print
 */

/** Mutable shared state used by other frontend modules. */
export const state = {
  /** Current frame rate setting */
  frameRate: DEFAULT_FRAME_RATE_60FPS,
  /** HTML element ID for the main rendering canvas */
  canvasId: undefined,
  /** HTML element ID for the UI controls container */
  uiControlsId: undefined,
  /** HTML element ID for the output/console display */
  outputId: undefined,
  /** UI manager instance for handling user interface components */
  uiManager: undefined,
  /** Flag indicating if UI canvas settings have changed */
  uiCanvasChanged: false,
  /** Three.js modules container for 3D rendering */
  threeJsModules: {},
  /** Graphics manager instance (either 2D or 3D) */
  graphicsManager: undefined,
  /** Container ID for ThreeJS rendering context */
  containerId: undefined,
  /** Graphics configuration arguments */
  graphicsArgs: {},
  /** Mirror canvas for main-thread video recording (receives frames from worker) */
  mirrorCanvas: null,
  /** Mirror canvas 2D context for drawing received frames */
  mirrorCanvasContext: null,
  /** VideoRecorder instance for main-thread recording */
  mainThreadVideoRecorder: null,
  /** Global reference to the current FastLEDAsyncController instance */
  fastLEDController: null,
  /** VideoRecorder instance used by non-worker path */
  videoRecorder: null,
  /**
   * Print function (will be overridden during initialization)
   * @param {...*} _args Arguments to print
   */
  print: function (..._args) {},
};
