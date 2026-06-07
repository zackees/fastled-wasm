// @ts-nocheck
/**
 * FastLED Audio Manager - Debug / Window API
 *
 * Installs the `window.*` debug helpers used for troubleshooting audio in the
 * browser. These functions delegate to the AudioManager singleton.
 */

/* eslint-disable no-console */
/* eslint-disable no-restricted-syntax */
/* eslint-disable max-len */
/* eslint-disable guard-for-in */

import { AUDIO_DEBUG, AUDIO_PROCESSOR_TYPES } from './audio_constants.ts';

/**
 * Install the audio debug `window.*` API surface. Called once at module load
 * from audio_manager.ts so that the global helpers are available regardless of
 * which consumer triggered the audio module to load.
 *
 * @param {Object} audioManager - The shared AudioManager instance
 */
export function installAudioDebugApi(audioManager) {
  /**
   * Make setupAudioAnalysis available globally
   * @param {HTMLAudioElement} audioElement - The audio element to analyze
   * @returns {Promise<Object>} Audio analysis components
   */
  window.setupAudioAnalysis = async function (audioElement) {
    return await audioManager.setupAudioAnalysis(audioElement);
  };

  /**
   * Get audio processor capabilities and status (debugging utility)
   * @returns {Object} Capabilities and status information
   */
  window.getAudioCapabilities = function () {
    const capabilities = audioManager.getCapabilities();
    console.log('🎵 Audio Engine Capabilities:', capabilities);
    return capabilities;
  };

  /**
   * Switch audio processor type (debugging utility)
   * @param {string} type - Processor type ('script_processor' or 'audio_worklet')
   * @returns {boolean} True if switch was successful
   */
  window.setAudioProcessor = function (type) {
    try {
      audioManager.setProcessorType(type);
      console.log(`🎵 Audio processor switched to: ${type}`);
      return true;
    } catch (error) {
      console.error('🎵 Failed to switch audio processor:', error);
      return false;
    }
  };

  /**
   * Use the best available audio processor (debugging utility)
   * @returns {string} The processor type that was selected
   */
  window.useBestAudioProcessor = function () {
    const isWorklet = audioManager.useAudioWorkletIfSupported();
    const selected = audioManager.getProcessorType();
    console.log(`🎵 Selected best audio processor: ${selected} (AudioWorklet: ${isWorklet})`);
    return selected;
  };

  /**
   * Force AudioWorklet mode (for testing - will fallback automatically if it fails)
   * @returns {string} The processor type that was set
   */
  window.forceAudioWorklet = function () {
    audioManager.setProcessorType(AUDIO_PROCESSOR_TYPES.AUDIO_WORKLET);
    console.log('🎵 Forced AudioWorklet mode (with automatic ScriptProcessor fallback)');
    return audioManager.getProcessorType();
  };

  /**
   * Force ScriptProcessor mode (for compatibility testing)
   * @returns {string} The processor type that was set
   */
  window.forceScriptProcessor = function () {
    audioManager.setProcessorType(AUDIO_PROCESSOR_TYPES.SCRIPT_PROCESSOR);
    console.log('🎵 Forced ScriptProcessor mode');
    return audioManager.getProcessorType();
  };

  /**
   * Enable audio debug logging (for troubleshooting)
   * @param {boolean} enabled - Whether to enable debug logging
   */
  window.setAudioDebug = function (enabled = true) {
    AUDIO_DEBUG.enabled = enabled;
    console.log(`🎵 Audio debug logging ${enabled ? 'enabled' : 'disabled'}`);
    return AUDIO_DEBUG.enabled;
  };

  /**
   * Get current audio debug settings
   * @returns {Object} Current debug configuration
   */
  window.getAudioDebugSettings = function () {
    console.log('🎵 Audio Debug Settings:', AUDIO_DEBUG);
    return AUDIO_DEBUG;
  };

  /**
   * Test AudioWorklet path loading (diagnostic tool)
   * @param {string} customPath - Optional custom path to test
   * @returns {Promise<boolean>} True if path loads successfully
   */
  window.testAudioWorkletPath = async function (customPath = null) {
    console.log('🎵 🔍 Testing AudioWorklet Path Loading...');

    const AudioContext = window.AudioContext || window.webkitAudioContext;
    const testContext = new AudioContext();

    const pathsToTest = customPath ? [customPath] : [
      './audio_worklet_processor.js',
      'audio_worklet_processor.js',
      '../audio_worklet_processor.js',
      'src/platforms/wasm/compiler/modules/audio_worklet_processor.js',
    ];

    for (const path of pathsToTest) {
      try {
        console.log(`🎵 🧪 Testing path: ${path}`);

        // First, try a simple fetch to see if the file exists
        try {
          // deno-lint-ignore no-await-in-loop
          const fetchResponse = await fetch(path);
          console.log(`🎵    📡 Fetch status: ${fetchResponse.status} ${fetchResponse.statusText}`);

          if (!fetchResponse.ok) {
            console.log(`🎵    ❌ Fetch failed: HTTP ${fetchResponse.status}`);
            continue;
          }

          console.log('🎵    ✅ File exists and is accessible');
        } catch (fetchError) {
          console.log(`🎵    ❌ Fetch error: ${fetchError.message}`);
          continue;
        }

        // Now try loading as AudioWorklet module
        // deno-lint-ignore no-await-in-loop
        await testContext.audioWorklet.addModule(path);
        console.log('🎵    🎵 ✅ AudioWorklet module loaded successfully!');

        testContext.close();
        return true;
      } catch (error) {
        console.log(`🎵    🎵 ❌ AudioWorklet loading failed: ${error.message}`);
      }
    }

    console.log('🎵 💡 Check browser Network tab for 404s or CORS errors');
    testContext.close();
    return false;
  };

  /**
   * Get information about the current page and potential AudioWorklet issues
   * @returns {Object} Environment information
   */
  window.getAudioWorkletEnvironmentInfo = function () {
    const info = {
      currentURL: window.location.href,
      protocol: window.location.protocol,
      host: window.location.host,
      pathname: window.location.pathname,
      isSecureContext: self.isSecureContext,
      audioWorkletSupported: 'audioWorklet'
        in (window.AudioContext || window.webkitAudioContext).prototype,
      userAgent: navigator.userAgent,
    };

    console.log('🎵 🌍 AudioWorklet Environment Information');
    console.log('🎵 Current URL:', info.currentURL);
    console.log('🎵 Protocol:', info.protocol);
    console.log('🎵 AudioWorklet API Supported:', info.audioWorkletSupported);

    // Check for common issues
    if (info.protocol === 'file:') {
      console.log('🎵 ⚠️  Running from file:// protocol - AudioWorklet may not work');
      console.log('🎵 💡 Solution: Use a local web server (python -m http.server, etc.)');
    }

    if (!info.audioWorkletSupported) {
      console.log('🎵 ❌ AudioWorklet API not supported in this browser');
    }

    return info;
  };
}
