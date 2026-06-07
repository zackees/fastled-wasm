// @ts-nocheck
/// <reference path="types.d.ts" />

/**
 * Video recording initialization for the FastLED frontend.
 *
 * Wires up the record button to either:
 *   - direct main-thread capture from the main canvas (when supported), or
 *   - worker-driven frame transfer through a hidden mirror canvas, or
 *   - the legacy non-worker path that captures from the main canvas directly.
 *
 * Also exposes the recorder helpers on `window` for debugging and external
 * control. Call `initVideoRecording()` to install the worker-event hook
 * and the timeout fallback that drives initial setup.
 */

import { VideoRecorder } from './modules/recording/video_recorder.ts';
import { state } from './state.ts';

/**
 * Initializes the video recorder with canvas and audio context
 */
export function initializeVideoRecorder() {
  // Wait for DOM to be ready
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', initializeVideoRecorder);
    return;
  }

  const recordButton = document.getElementById('record-btn');

  if (!recordButton) {
    console.warn('Record button not found, video recording disabled');
    return;
  }

  // Check if worker mode is active - if so, use worker-based recording
  if (window.fastLEDWorkerManager && window.fastLEDWorkerManager.isWorkerActive) {
    console.log('Worker mode active - using worker-based video recording');
    initializeWorkerVideoRecorder(recordButton);
    return;
  }

  // For non-worker mode, wait for canvas to be ready
  const canvas = document.getElementById('myCanvas');
  if (!canvas) {
    console.warn('Canvas not found, video recording disabled');
    return;
  }

  // Wait for canvas to be properly initialized with graphics manager
  let retryCount = 0;
  const maxRetries = 30; // Max 6 seconds of retrying
  let initialized = false; // Track if initialization succeeded

  const tryInitialize = () => {
    // Stop retrying if already initialized
    if (initialized) {
      return;
    }

    try {
      // Check if worker mode became active during retry period
      if (window.fastLEDWorkerManager && window.fastLEDWorkerManager.isWorkerActive) {
        console.log('Worker mode became active - switching to worker-based recording');
        initializeWorkerVideoRecorder(recordButton);
        initialized = true; // Mark as successfully initialized
        return;
      }

      // Check if graphics manager has been initialized (this is the real dependency)
      if (typeof window.graphicsManager === 'undefined' && typeof state.graphicsManager === 'undefined') {
        throw new Error('Graphics manager not initialized yet');
      }

      // Validate canvas element exists (without creating conflicting context)
      if (!canvas || !canvas.getContext) {
        throw new Error('Canvas not ready yet');
      }

      actuallyInitializeVideoRecorder(canvas, recordButton);
      initialized = true; // Mark as successfully initialized
    } catch (error) {
      retryCount++;
      if (retryCount < maxRetries) {
        setTimeout(tryInitialize, 200);
      } else {
        console.warn('Failed to initialize video recorder - canvas/graphics not ready after maximum retries');
        recordButton.style.display = 'none';
      }
    }
  };

  // Start trying to initialize after a short delay
  setTimeout(tryInitialize, 1000);
}

/**
 * Creates a hidden mirror canvas to receive frames from worker for main-thread recording
 * @param {number} width - Canvas width
 * @param {number} height - Canvas height
 * @returns {HTMLCanvasElement} Mirror canvas
 */
export function createMirrorCanvas(width, height) {
  console.log('Creating mirror canvas:', width, 'x', height);

  // Create canvas element
  const canvas = document.createElement('canvas');
  canvas.id = 'mirror-canvas';
  canvas.width = width;
  canvas.height = height;

  // Hide canvas (not displayed, only used for recording)
  canvas.style.position = 'absolute';
  canvas.style.left = '-9999px';
  canvas.style.top = '-9999px';
  canvas.style.pointerEvents = 'none';

  // Add to DOM (required for MediaRecorder)
  document.body.appendChild(canvas);

  console.log('Mirror canvas created and added to DOM');

  return canvas;
}

/**
 * Sets up message listener for frame updates from worker
 */
export function setupWorkerFrameListener() {
  if (!window.fastLEDWorkerManager || !window.fastLEDWorkerManager.worker) {
    console.error('Worker not available for frame listener setup');
    return;
  }

  // Add custom handler for frame_update messages
  const originalHandler = window.fastLEDWorkerManager.worker.onmessage;

  window.fastLEDWorkerManager.worker.onmessage = (event) => {
    const { type, payload } = event.data;

    if (type === 'frame_update' && payload && payload.bitmap) {
      handleWorkerFrameUpdate(payload);
      return; // Don't pass to original handler
    }

    // Pass other messages to original handler
    if (originalHandler) {
      originalHandler(event);
    }
  };

  console.log('Worker frame listener setup complete');
}

/**
 * Handles frame update from worker - draws to mirror canvas
 * @param {Object} payload - Frame data with ImageBitmap
 */
export function handleWorkerFrameUpdate(payload) {
  const { bitmap, width, height } = payload;

  try {
    // Create mirror canvas if needed or resize if dimensions changed
    if (!state.mirrorCanvas || state.mirrorCanvas.width !== width || state.mirrorCanvas.height !== height) {
      console.log('Creating/resizing mirror canvas:', width, 'x', height);

      if (state.mirrorCanvas) {
        document.body.removeChild(state.mirrorCanvas);
      }

      state.mirrorCanvas = createMirrorCanvas(width, height);
      state.mirrorCanvasContext = state.mirrorCanvas.getContext('2d', { willReadFrequently: false });
    }

    // Draw bitmap to mirror canvas (hardware-accelerated)
    if (state.mirrorCanvasContext) {
      state.mirrorCanvasContext.drawImage(bitmap, 0, 0);
    }

    // Bitmap is transferred, no need to close it
  } catch (error) {
    console.error('Error drawing frame to mirror canvas:', error);
  }
}

/**
 * Initializes main-thread video recording with worker frame transfer
 * Worker captures frames as ImageBitmap and sends to main thread
 * Main thread draws to mirror canvas and uses MediaRecorder (where it's available)
 * @param {HTMLElement} recordButton - The record button element
 */
export function initializeWorkerVideoRecorder(recordButton) {
  console.log('Initializing video recorder for worker mode');

  // Check MediaRecorder support
  if (typeof MediaRecorder === 'undefined') {
    console.warn('MediaRecorder API not supported, video recording disabled');
    recordButton.style.display = 'none';
    return;
  }

  // Make record button visible
  recordButton.classList.add('visible');
  console.log('Record button made visible');

  // Try direct capture from the main canvas. After transferControlToOffscreen(),
  // the OffscreenCanvas content auto-syncs to the original HTMLCanvasElement.
  // captureStream() can capture this displayed content directly, eliminating the
  // expensive ImageBitmap→postMessage→mirror canvas pipeline.
  const mainCanvas = document.getElementById('myCanvas');
  let useDirectCapture = false;

  if (mainCanvas) {
    try {
      const testStream = /** @type {HTMLCanvasElement} */ (mainCanvas).captureStream(0);
      if (testStream && testStream.getVideoTracks().length > 0) {
        testStream.getTracks().forEach((t) => t.stop());
        useDirectCapture = true;
        console.log('Direct canvas capture supported - using optimized recording pipeline (no ImageBitmap transfer)');
      }
    } catch (e) {
      console.log('Direct canvas capture not supported, falling back to mirror canvas:', e.message);
    }
  }

  if (!useDirectCapture) {
    // Fall back to mirror canvas approach (worker sends ImageBitmap frames)
    setupWorkerFrameListener();
  }

  let isRecording = false;

  /**
   * Updates record button UI state
   * @param {boolean} recording - Whether currently recording
   */
  function updateRecordButtonUI(recording) {
    isRecording = recording;

    if (recording) {
      recordButton.classList.add('recording');
      const recordIcon = /** @type {HTMLElement|null} */ (recordButton.querySelector('.record-icon'));
      const stopIcon = /** @type {HTMLElement|null} */ (recordButton.querySelector('.stop-icon'));
      if (recordIcon) recordIcon.style.display = 'none';
      if (stopIcon) stopIcon.style.display = 'block';
    } else {
      recordButton.classList.remove('recording');
      const recordIcon = /** @type {HTMLElement|null} */ (recordButton.querySelector('.record-icon'));
      const stopIcon = /** @type {HTMLElement|null} */ (recordButton.querySelector('.stop-icon'));
      if (recordIcon) recordIcon.style.display = 'block';
      if (stopIcon) stopIcon.style.display = 'none';
    }

    if (window.updateRecordButtonTooltip) {
      window.updateRecordButtonTooltip();
    }
  }

  // Handle record button click
  recordButton.addEventListener('click', async () => {
    if (!window.fastLEDWorkerManager || !window.fastLEDWorkerManager.isWorkerActive) {
      console.error('Worker not active, cannot record');
      return;
    }

    try {
      if (!isRecording) {
        // === START RECORDING ===
        let recordingCanvas;
        let recordingFps = 60;

        if (useDirectCapture && mainCanvas) {
          // Direct capture: record from the main canvas directly.
          // Worker renders to OffscreenCanvas which auto-syncs to the HTMLCanvasElement.
          // No ImageBitmap creation, no postMessage transfer, no mirror canvas needed.
          recordingCanvas = mainCanvas;
          console.log('Starting direct canvas capture recording');
        } else {
          // Mirror canvas fallback: tell worker to start frame capture
          const response = await window.fastLEDWorkerManager.sendMessageWithResponse({
            type: 'start_recording',
            payload: {
              fps: 60,
              settings: window.getVideoSettings ? window.getVideoSettings() : {}
            }
          });

          if (!response.success) {
            throw new Error('Worker failed to start frame capture');
          }

          console.log('Worker frame capture started:', response);
          recordingFps = response.fps || 60;

          // Wait for mirror canvas to be created (give frames time to arrive)
          await new Promise((resolve) => {
            setTimeout(resolve, 500);
          });

          if (!state.mirrorCanvas) {
            throw new Error('Mirror canvas not created - no frames received from worker');
          }

          recordingCanvas = state.mirrorCanvas;
        }

        // Create VideoRecorder with the chosen canvas
        const defaultSettings = window.getVideoSettings ? window.getVideoSettings() : {};

        state.mainThreadVideoRecorder = new VideoRecorder({
          canvas: recordingCanvas,
          audioContext: null,
          fps: recordingFps,
          settings: {
            ...defaultSettings,
            fps: undefined
          },
          onStateChange: updateRecordButtonUI
        });

        state.mainThreadVideoRecorder.startRecording();
        console.log('Recording started with codec:', state.mainThreadVideoRecorder.selectedMimeType,
          useDirectCapture ? '(direct capture)' : '(mirror canvas)');
      } else {
        // === STOP RECORDING ===
        if (state.mainThreadVideoRecorder) {
          state.mainThreadVideoRecorder.stopRecording();
        }

        // Only tell worker to stop frame capture if using mirror canvas mode
        if (!useDirectCapture) {
          await window.fastLEDWorkerManager.sendMessageWithResponse({
            type: 'stop_recording',
            payload: {}
          });
        }

        console.log('Recording stopped');
      }
    } catch (error) {
      console.error('Recording error:', error);
      isRecording = false;
      recordButton.classList.remove('recording');
    }
  });

  console.log('Video recorder initialized:', useDirectCapture ? 'direct canvas capture' : 'mirror canvas fallback');
}

/**
 * Actually initializes the video recorder once canvas is ready
 */
export function actuallyInitializeVideoRecorder(canvas, recordButton) {
  // Note: Audio context removed for async video recorder
  // Audio recording will be handled separately if needed

  // Check if MediaRecorder is supported
  if (typeof MediaRecorder === 'undefined') {
    console.warn('MediaRecorder API not supported, video recording disabled');
    recordButton.style.display = 'none';
    return;
  }

  // Always use default settings (no localStorage persistence)
  const defaultSettings = window.getVideoSettings ? window.getVideoSettings() : null;

  try {
    // Validate canvas is ready (without creating a context that would conflict with graphics manager)
    if (!canvas.getContext) {
      throw new Error('Canvas does not support getContext method');
    }

    // Don't create a context here - the graphics manager handles context creation
    // Just validate the canvas element is ready for use

    // Create video recorder instance with optimized settings
    state.videoRecorder = new VideoRecorder({
      canvas,
      audioContext: null, // Disable audio for better performance
      fps: defaultSettings?.fps || 60,
      settings: {
        // Use default settings but exclude fps to avoid conflicts
        ...defaultSettings,
        fps: undefined, // Remove fps from settings to ensure constructor fps parameter takes precedence
      },
      onStateChange: (isRecording) => {
        // Update button visual state
        if (isRecording) {
          recordButton.classList.add('recording');
          // Update icon
          const recordIcon = recordButton.querySelector('.record-icon');
          const stopIcon = recordButton.querySelector('.stop-icon');
          if (recordIcon) recordIcon.style.display = 'none';
          if (stopIcon) stopIcon.style.display = 'block';
        } else {
          recordButton.classList.remove('recording');
          // Update icon
          const recordIcon = recordButton.querySelector('.record-icon');
          const stopIcon = recordButton.querySelector('.stop-icon');
          if (recordIcon) recordIcon.style.display = 'block';
          if (stopIcon) stopIcon.style.display = 'none';
        }

        // Update tooltip with current encoding format
        // Use setTimeout to ensure the state change is complete
        setTimeout(() => {
          if (window.updateRecordButtonTooltip) {
            window.updateRecordButtonTooltip();
          }
        }, 0);
      },
    });

    // Expose video recorder globally for settings updates
    window.videoRecorder = state.videoRecorder;

    // Create custom tooltip element
    const tooltip = document.createElement('div');
    tooltip.id = 'record-tooltip';
    tooltip.style.cssText = `
      position: absolute;
      background: rgba(0, 0, 0, 0.9);
      color: white;
      padding: 8px 12px;
      border-radius: 4px;
      font-size: 14px;
      font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
      white-space: nowrap;
      z-index: 10000;
      pointer-events: none;
      display: none;
      box-shadow: 0 2px 8px rgba(0, 0, 0, 0.3);
    `;
    document.body.appendChild(tooltip);

    // Function to update record button tooltip with current encoding format
    function updateRecordButtonTooltip() {
      if (state.videoRecorder && recordButton) {
        const codecName = state.videoRecorder.getCodecDisplayName();
        const isRecording = state.videoRecorder.getIsRecording();

        let tooltipText;
        if (isRecording) {
          tooltipText = `Stop Recording (${codecName})`;
        } else {
          tooltipText = `Record in ${codecName}`;
        }

        // Store tooltip text for mouse events
        recordButton.setAttribute('data-tooltip', tooltipText);
        // Remove browser default tooltip
        recordButton.removeAttribute('title');
      }
    }

    // Add mouse events for immediate tooltip display
    recordButton.addEventListener('mouseenter', (e) => {
      const tooltipText = e.target.getAttribute('data-tooltip');
      if (tooltipText) {
        tooltip.textContent = tooltipText;
        tooltip.style.display = 'block';

        // Position tooltip above the button
        const rect = recordButton.getBoundingClientRect();
        tooltip.style.left = `${rect.left + rect.width / 2}px`;
        tooltip.style.top = `${rect.top - tooltip.offsetHeight - 8}px`;

        // Adjust horizontal position if tooltip would go off-screen
        const tooltipRect = tooltip.getBoundingClientRect();
        if (tooltipRect.right > window.innerWidth - 10) {
          tooltip.style.left = `${window.innerWidth - tooltipRect.width - 10}px`;
        }
        if (tooltipRect.left < 10) {
          tooltip.style.left = '10px';
        }
      }
    });

    recordButton.addEventListener('mouseleave', () => {
      tooltip.style.display = 'none';
    });

    recordButton.addEventListener('mousemove', () => {
      // Update position on mouse move for better tracking
      const rect = recordButton.getBoundingClientRect();
      tooltip.style.left = `${rect.left + rect.width / 2}px`;
      tooltip.style.top = `${rect.top - tooltip.offsetHeight - 8}px`;

      // Center tooltip horizontally relative to button
      const tooltipRect = tooltip.getBoundingClientRect();
      const centerOffset = tooltipRect.width / 2;
      tooltip.style.left = `${rect.left + rect.width / 2 - centerOffset}px`;
    });

    // Update tooltip initially
    updateRecordButtonTooltip();

    // Expose globally so it can be called when settings change
    window.updateRecordButtonTooltip = updateRecordButtonTooltip;

    // Add click handler to record button
    recordButton.addEventListener('click', (e) => {
      e.preventDefault();
      if (state.videoRecorder) {
        state.videoRecorder.toggleRecording();
      }
    });

    // Add keyboard shortcut (Ctrl+R or Cmd+R)
    document.addEventListener('keydown', (e) => {
      if ((e.ctrlKey || e.metaKey) && e.key === 'r' && !e.shiftKey) {
        e.preventDefault();
        if (state.videoRecorder) {
          state.videoRecorder.toggleRecording();
        }
      }
    });

    console.log('Optimized video recorder initialized (performance settings applied)');
  } catch (error) {
    console.error('Failed to initialize video recorder:', error);
    recordButton.style.display = 'none';
  }
}

/**
 * Wires up the video recorder auto-initialization triggers.
 *
 * Prefers an event-based path (worker:initialized) for faster response,
 * with a 500ms timeout fallback for cases where the worker fails to start
 * or events never fire. Also exposes the recorder helpers on `window`.
 */
export function initVideoRecording() {
  // Initialize video recorder after FastLED is ready
  // Prefer event-based initialization for faster response, with timeout fallback
  let videoRecorderInitialized = false;

  // Listen for worker:initialized event for immediate initialization
  if (typeof window !== 'undefined' && window.fastLEDEvents) {
    window.fastLEDEvents.on('worker:initialized', () => {
      if (!videoRecorderInitialized) {
        console.log('Worker initialized event received, initializing video recorder immediately');
        videoRecorderInitialized = true;
        initializeVideoRecorder();
      }
    });
  }

  // Fallback timeout in case worker mode fails or events don't fire
  // Reduced from 2000ms to 500ms for better responsiveness
  setTimeout(() => {
    if (!videoRecorderInitialized) {
      console.log('Initializing video recorder via timeout fallback (worker event not received)');
      videoRecorderInitialized = true;
      initializeVideoRecorder();
    }
  }, 500); // Reduced timeout - worker initializes at ~200ms, 500ms gives buffer

  // Expose video recorder functions globally for debugging
  window.getVideoRecorder = () => state.videoRecorder;
  window.startVideoRecording = () => state.videoRecorder?.startRecording();
  window.stopVideoRecording = () => state.videoRecorder?.stopRecording();
  window.testVideoRecording = () => state.videoRecorder?.testRecording();
}
