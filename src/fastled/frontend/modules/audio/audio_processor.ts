// @ts-nocheck
/**
 * FastLED Audio Manager - Processor Classes
 *
 * Audio processor implementations and factory for selecting the best
 * available processor (ScriptProcessor vs AudioWorklet).
 */

/* eslint-disable no-console */
/* eslint-disable no-restricted-syntax */
/* eslint-disable max-len */
/* eslint-disable guard-for-in */

import { AUDIO_DEBUG, AUDIO_PROCESSOR_TYPES, AUDIO_SAMPLE_BLOCK_SIZE } from './audio_constants.ts';

/**
 * TIMESTAMP IMPLEMENTATION DOCUMENTATION:
 *
 * Audio sample timestamps are relative to the start of the audio file, not absolute time.
 * This ensures consistent timing that's meaningful for audio synchronization.
 *
 * Priority order for timestamp sources:
 * 1. audioElement.currentTime - Preferred: gives playback position in audio file (seconds → milliseconds)
 * 2. audioContext.currentTime - Fallback: high-precision audio context time (seconds → milliseconds)
 * 3. performance.now() - Final fallback: high-resolution system time relative to page load
 *
 * This approach ensures that audio-visual synchronization remains accurate regardless
 * of when playback starts or system performance variations.
 */

/**
 * Abstract base class for audio processors
 * Provides a common interface for different audio processing implementations
 * Enables polymorphic usage of ScriptProcessor and AudioWorklet implementations
 */
export class AudioProcessor {
  /**
   * Creates a new AudioProcessor instance
   * @param {AudioContext} audioContext - Web Audio API context
   * @param {Function} sampleCallback - Callback function for processed audio samples
   */
  constructor(audioContext, sampleCallback) {
    /** @type {AudioContext} Web Audio API context */
    this.audioContext = audioContext;

    /** @type {Function} Callback for processed audio samples */
    this.sampleCallback = sampleCallback;

    /** @type {boolean} Whether audio processing is currently active */
    this.isProcessing = false;
  }

  /**
   * Initialize the audio processor
   * @abstract
   * @param {MediaElementAudioSourceNode | MediaStreamAudioSourceNode} [_source] - Optional audio source node
   * @returns {Promise<void>}
   */
  initialize(_source) {
    // Base class method - returns rejected promise since it must be implemented by subclass
    return Promise.reject(new Error('initialize() must be implemented by subclass'));
  }

  /**
   * Start audio processing
   */
  start() {
    this.isProcessing = true;
  }

  /**
   * Stop audio processing
   */
  stop() {
    this.isProcessing = false;
  }

  /**
   * Clean up resources
   */
  cleanup() {
    this.stop();
  }

  /**
   * Get the processor type identifier
   * @abstract
   * @returns {string} Processor type string
   */
  getType() {
    throw new Error('getType() must be implemented by subclass');
  }
}

/**
 * ScriptProcessor-based audio processor (legacy but widely supported)
 * Uses the deprecated ScriptProcessorNode for broad browser compatibility
 * Runs on the main thread which can cause performance issues but works everywhere
 */
export class ScriptProcessorAudioProcessor extends AudioProcessor {
  /**
   * Creates a new ScriptProcessorAudioProcessor instance
   * @param {AudioContext} audioContext - Web Audio API context
   * @param {Function} sampleCallback - Callback function for processed audio samples
   */
  constructor(audioContext, sampleCallback) {
    super(audioContext, sampleCallback);

    /** @type {ScriptProcessorNode|null} The ScriptProcessor node */
    this.scriptNode = null;

    /** @type {Int16Array} Buffer for converting audio samples to int16 format */
    this.sampleBuffer = new Int16Array(AUDIO_SAMPLE_BLOCK_SIZE);
  }

  /**
   * Initialize the ScriptProcessor node and audio processing chain
   * @param {MediaElementAudioSourceNode | MediaStreamAudioSourceNode} source - Audio source node to connect
   * @returns {Promise<void>}
   */
  initialize(source) {
    // Create script processor node - returns promise for base class compatibility
    this.scriptNode = this.audioContext.createScriptProcessor(AUDIO_SAMPLE_BLOCK_SIZE, 1, 1);

    // Set up audio processing callback
    this.scriptNode.onaudioprocess = (audioProcessingEvent) => {
      if (!this.isProcessing) return;

      // Get input data from the left channel
      const { inputBuffer } = audioProcessingEvent;
      const inputData = inputBuffer.getChannelData(0);

      // Convert float32 audio data to int16 range
      this.convertAudioSamples(inputData, this.sampleBuffer);

      // Get timestamp
      const timestamp = this.getTimestamp();

      // Call the sample callback
      this.sampleCallback(this.sampleBuffer, timestamp);
    };

    // Connect nodes
    source.connect(this.scriptNode);
    this.scriptNode.connect(this.audioContext.destination);

    // Return resolved promise for interface compatibility
    return Promise.resolve();
  }

  /**
   * Convert audio samples from float32 to int16 format
   * @param {Float32Array} inputData - Input audio data in float32 format (-1.0 to 1.0)
   * @param {Int16Array} sampleBuffer - Output buffer for int16 samples (-32768 to 32767)
   */
  convertAudioSamples(inputData, sampleBuffer) {
    for (let i = 0; i < inputData.length; i++) {
      // Convert from float32 (-1.0 to 1.0) to int16 range (-32768 to 32767)
      sampleBuffer[i] = Math.floor(inputData[i] * 32767);
    }
  }

  /**
   * Get current timestamp for audio synchronization
   * @returns {number} Timestamp in milliseconds
   */
  getTimestamp() {
    // Use AudioContext.currentTime as primary source for ScriptProcessor
    return Math.floor(this.audioContext.currentTime * 1000);
  }

  /**
   * Clean up ScriptProcessor resources
   */
  cleanup() {
    super.cleanup();
    if (this.scriptNode) {
      this.scriptNode.onaudioprocess = null;
      this.scriptNode.disconnect();
      this.scriptNode = null;
    }
  }

  /**
   * Get the processor type identifier
   * @returns {string} Processor type
   */
  getType() {
    return AUDIO_PROCESSOR_TYPES.SCRIPT_PROCESSOR;
  }
}

/**
 * AudioWorklet-based audio processor (modern, runs on audio thread)
 * Provides better performance and timing consistency than ScriptProcessor
 */
export class AudioWorkletAudioProcessor extends AudioProcessor {
  constructor(audioContext, sampleCallback) {
    super(audioContext, sampleCallback);
    this.workletNode = null;
    this.isWorkletLoaded = false;
    console.log('🎵 AudioWorklet processor created');
  }

  /**
   * Initialize the AudioWorklet processor and audio processing chain
   * @param {MediaElementAudioSourceNode | MediaStreamAudioSourceNode} source - Audio source node to connect
   * @returns {Promise<void>}
   */
  async initialize(source) {
    try {
      // Load the AudioWorklet module if not already loaded
      if (!this.isWorkletLoaded) {
        // Try different possible paths for the AudioWorklet processor
        const possiblePaths = [
          './audio_worklet_processor.js',
          'audio_worklet_processor.js',
          '../audio_worklet_processor.js',
          'src/platforms/wasm/compiler/modules/audio_worklet_processor.js',
        ];

        let loadSuccess = false;
        const diagnosticInfo = [];

        for (const path of possiblePaths) {
          try {
            // deno-lint-ignore no-await-in-loop
            await this.audioContext.audioWorklet.addModule(path);
            console.log(`🎵 ✅ AudioWorklet module loaded successfully from: ${path}`);
            loadSuccess = true;
            break;
          } catch (pathError) {
            // Collect detailed diagnostic information
            const diagnostic = {
              path,
              error: pathError.message,
              errorName: pathError.name,
              errorType: this.diagnoseAudioWorkletError(pathError, path),
            };
            diagnosticInfo.push(diagnostic);

            console.warn(`🎵 ❌ Failed to load AudioWorklet from ${path}:`, pathError.message);
            console.warn(`🎵 🔍 Error type: ${diagnostic.errorType}`);
          }
        }

        // If all paths failed, show diagnostic information
        if (!loadSuccess) {
          console.log('🎵 📊 AudioWorklet Loading Diagnostic Report');
          diagnosticInfo.forEach((info, index) => {
            console.log(`🎵 📁 Attempt ${index + 1}: ${info.path}`);
            console.log(`🎵    Error: ${info.error}`);
            console.log(`🎵    Type: ${info.errorType}`);
          });
          console.log('🎵 💡 Check browser Network tab for specific HTTP status codes');
        }

        if (!loadSuccess) {
          // Provide a summary of the diagnostic information
          const errorTypes = [...new Set(diagnosticInfo.map((d) => d.errorType))];
          const detailedError = new Error(`
🎵 AudioWorklet module could not be loaded from any path.

Diagnosed error types: ${errorTypes.join(', ')}

Run these diagnostic commands in the browser console:
  window.testAudioWorkletPath()              - Test all paths
  window.getAudioWorkletEnvironmentInfo()    - Check environment

Quick fixes to try:
1. Copy audio_worklet_processor.js to the same directory as this page
2. Check browser Network tab for 404 or CORS errors
3. Ensure you're using http:// or https:// (not file://)
4. Verify web server is serving .js files correctly

The system will automatically fall back to ScriptProcessor.`);

          throw detailedError;
        }

        this.isWorkletLoaded = true;
      }

      // Create the AudioWorklet node
      this.workletNode = new AudioWorkletNode(this.audioContext, 'fastled-audio-processor', {
        numberOfInputs: 1,
        numberOfOutputs: 1,
        outputChannelCount: [1],
        processorOptions: {
          sampleRate: this.audioContext.sampleRate,
        },
      });

      // Send init config to worklet processor
      this.workletNode.port.postMessage({
        type: 'init',
        data: {
          sampleRate: this.audioContext.sampleRate,
          bufferSize: 512,
        },
      });

      // Set up message handling from the worklet
      this.workletNode.port.onmessage = (event) => {
        this.handleWorkletMessage(event.data);
      };

      // Handle worklet errors
      this.workletNode.onprocessorerror = (error) => {
        console.error('🎵 AudioWorklet processor error:', error);
      };

      // Connect nodes: source -> worklet -> destination
      source.connect(this.workletNode);
      this.workletNode.connect(this.audioContext.destination);
    } catch (error) {
      console.error('🎵 Failed to initialize AudioWorklet processor:', error);

      // Provide helpful error messages for common issues
      if (error.name === 'NotSupportedError') {
        console.error('🎵 AudioWorklet is not supported in this browser');
      } else if (error.message.includes('audio_worklet_processor.js')) {
        console.error('🎵 Could not load audio_worklet_processor.js - check file path');
      }

      throw error;
    }
  }

  /**
   * Handle messages from the AudioWorklet
   * @param {Object} data - Message data from worklet
   */
  handleWorkletMessage(message) {
    const { type, data } = message;
    // The worklet sends: { type: 'audioData', data: { samples, timestamp, ... } }
    const samples = data?.samples;
    const timestamp = data?.timestamp;

    switch (type) {
      case 'audioData':
        if (this.isProcessing && samples && samples.length > 0) {
          // Convert samples array back to Int16Array for compatibility
          const sampleBuffer = new Int16Array(samples);

          // Call the sample callback with enhanced timestamp
          const enhancedTimestamp = this.enhanceTimestamp(timestamp);
          this.sampleCallback(sampleBuffer, enhancedTimestamp);
        }
        break;

      case 'error':
        console.error('🎵 AudioWorklet reported error:', data.message);
        break;

      case 'debug':
        // Only log debug messages when debugging is enabled
        if (AUDIO_DEBUG.enabled && Math.random() < AUDIO_DEBUG.workletRate) {
          console.log('🎵 AudioWorklet debug:', data.message);
        }
        break;

      default:
        console.warn('🎵 Unknown message type from AudioWorklet:', type);
    }
  }

  /**
   * Enhance the timestamp from AudioWorklet with additional context
   * @param {number} workletTimestamp - Timestamp from AudioWorklet (audioContext.currentTime)
   * @returns {number} Enhanced timestamp in milliseconds
   */
  enhanceTimestamp(workletTimestamp) {
    // AudioWorklet provides high-precision AudioContext.currentTime
    // Convert from seconds to milliseconds for consistency
    return Math.floor(workletTimestamp);
  }

  start() {
    super.start();
    if (this.workletNode) {
      console.log('🎵 Starting AudioWorklet processing');
      this.workletNode.port.postMessage({
        type: 'start',
        timestamp: this.audioContext.currentTime,
      });
    }
  }

  stop() {
    super.stop();
    if (this.workletNode) {
      console.log('🎵 Stopping AudioWorklet processing');
      this.workletNode.port.postMessage({
        type: 'stop',
        timestamp: this.audioContext.currentTime,
      });
    }
  }

  /**
   * Send configuration to the AudioWorklet
   * @param {Object} config - Configuration object
   */
  sendConfig(config) {
    if (this.workletNode) {
      console.log('🎵 Sending config to AudioWorklet:', config);
      this.workletNode.port.postMessage({
        type: 'config',
        data: config,
        timestamp: this.audioContext.currentTime,
      });
    }
  }

  cleanup() {
    super.cleanup();

    if (this.workletNode) {
      console.log('🎵 Cleaning up AudioWorklet processor');

      try {
        // Stop processing
        this.workletNode.port.postMessage({ type: 'stop' });

        // Clear message handler
        this.workletNode.port.onmessage = null;
        this.workletNode.onprocessorerror = null;

        // Disconnect the node
        this.workletNode.disconnect();

        console.log('🎵 AudioWorklet cleanup completed');
      } catch (error) {
        console.warn('🎵 Error during AudioWorklet cleanup:', error);
      }

      this.workletNode = null;
    }
  }

  /**
   * Diagnose the type of AudioWorklet loading error
   * @param {Error} error - The error that occurred
   * @param {string} [_path] - Optional path that failed to load
   * @returns {string} Error type description
   */
  diagnoseAudioWorkletError(error, _path) {
    const errorMsg = error.message.toLowerCase();
    const errorName = error.name;

    // Common error patterns and their likely causes
    if (errorMsg.includes('cors') || errorMsg.includes('cross-origin')) {
      return 'CORS_ERROR - Cross-origin request blocked';
    }

    if (errorMsg.includes('404') || errorMsg.includes('not found')) {
      return 'PATH_ERROR - File not found (404)';
    }

    if (errorMsg.includes('network') || errorMsg.includes('fetch')) {
      return 'NETWORK_ERROR - Network request failed';
    }

    if (errorMsg.includes('syntax') || errorMsg.includes('parse')) {
      return 'SYNTAX_ERROR - JavaScript syntax error in worklet file';
    }

    if (errorMsg.includes('security') || errorMsg.includes('insecure')) {
      return 'SECURITY_ERROR - Security restriction (HTTPS required?)';
    }

    if (errorMsg.includes('mime') || errorMsg.includes('content-type')) {
      return 'MIME_ERROR - Incorrect MIME type (should be application/javascript)';
    }

    if (errorName === 'TypeError') {
      return 'TYPE_ERROR - Likely a path resolution or module loading issue';
    }

    if (errorName === 'AbortError') {
      return 'ABORT_ERROR - Request was aborted (timeout or manual cancel)';
    }

    if (errorMsg.includes('unable to load') || errorMsg.includes('failed to load')) {
      return 'LOAD_ERROR - Generic loading failure (check network tab)';
    }

    return `UNKNOWN_ERROR - ${errorName}: Check browser console and network tab`;
  }

  getType() {
    return AUDIO_PROCESSOR_TYPES.AUDIO_WORKLET;
  }
}

/**
 * Factory for creating audio processors
 */
export class AudioProcessorFactory {
  /**
   * Create an audio processor of the specified type
   * @param {string} type - Processor type
   * @param {AudioContext} audioContext - Audio context
   * @param {Function} sampleCallback - Callback for audio samples
   * @returns {AudioProcessor}
   */
  static create(type, audioContext, sampleCallback) {
    switch (type) {
      case AUDIO_PROCESSOR_TYPES.SCRIPT_PROCESSOR:
        return new ScriptProcessorAudioProcessor(audioContext, sampleCallback);
      case AUDIO_PROCESSOR_TYPES.AUDIO_WORKLET:
        return new AudioWorkletAudioProcessor(audioContext, sampleCallback);
      default:
        console.warn(`Unknown audio processor type: ${type}, falling back to ScriptProcessor`);
        return new ScriptProcessorAudioProcessor(audioContext, sampleCallback);
    }
  }

  /**
   * Check if AudioWorklet is supported
   * @returns {boolean}
   */
  static isAudioWorkletSupported() {
    return 'audioWorklet' in AudioContext.prototype;
  }

  /**
   * Get the best available processor type
   * Note: This only checks API support, not actual file availability
   * @returns {string}
   */
  static getBestProcessorType() {
    if (this.isAudioWorkletSupported()) {
      return AUDIO_PROCESSOR_TYPES.AUDIO_WORKLET;
    }
    return AUDIO_PROCESSOR_TYPES.SCRIPT_PROCESSOR;
  }

  /**
   * Get a conservative processor type that's most likely to work
   * @returns {string}
   */
  static getReliableProcessorType() {
    // For now, always return ScriptProcessor as it's more reliable
    // Can be changed to return AUDIO_WORKLET when file loading is more robust
    return AUDIO_PROCESSOR_TYPES.SCRIPT_PROCESSOR;
  }
}
