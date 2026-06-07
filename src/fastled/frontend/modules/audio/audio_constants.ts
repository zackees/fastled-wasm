// @ts-nocheck
/**
 * FastLED Audio Manager - Constants
 *
 * Shared constants used across the audio manager modules.
 */

/* eslint-disable no-console */
/* eslint-disable no-restricted-syntax */
/* eslint-disable max-len */
/* eslint-disable guard-for-in */

/**
 * Audio sample block size configuration
 * Must match i2s read size on ESP32-C3 for compatibility
 * @constant {number}
 */
export const AUDIO_SAMPLE_BLOCK_SIZE = 512;

/**
 * Debug configuration for audio processing
 * Controls logging frequency to prevent console spam while maintaining visibility
 * @constant {Object}
 */
export const AUDIO_DEBUG = {
  /** @type {boolean} Enable/disable verbose debugging */
  enabled: false,
  /** @type {number} How often to log sample processing (0.1% of the time) */
  sampleRate: 0.001,
  /** @type {number} How often to log buffer operations (10% of the time) */
  bufferRate: 0.1,
  /** @type {number} How often to log worklet debug messages (0.01% of the time) */
  workletRate: 0.0001,
};

/**
 * Audio processor type constants
 * Defines available audio processing implementations
 * @constant {Object}
 */
export const AUDIO_PROCESSOR_TYPES = {
  /** @type {string} Legacy ScriptProcessor (main thread) */
  SCRIPT_PROCESSOR: 'script_processor',
  /** @type {string} Modern AudioWorklet (audio thread) */
  AUDIO_WORKLET: 'audio_worklet',
};
