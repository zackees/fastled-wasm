// @ts-nocheck
/**
 * FastLED Audio Manager Module
 *
 * Comprehensive audio processing system for FastLED WebAssembly applications.
 * Provides real-time audio analysis, file handling, and integration with LED effects.
 *
 * Key features:
 * - Multiple audio processor support (AudioWorklet and ScriptProcessor)
 * - Real-time audio sample processing and buffering
 * - Audio file upload and playback management
 * - Cross-browser compatibility with fallbacks
 * - High-precision timing and synchronization
 * - Memory-efficient sample storage and retrieval
 * - Debug logging and diagnostics
 * - Automatic processor selection based on browser capabilities
 *
 * Supported audio formats:
 * - MP3, WAV, OGG, AAC (browser-dependent)
 * - Real-time microphone input
 * - HTML audio element playback
 *
 * Processor types:
 * - AudioWorklet: Modern, high-performance (runs on audio thread)
 * - ScriptProcessor: Legacy fallback (runs on main thread)
 *
 * @module AudioManager
 */

/* eslint-disable no-console */
/* eslint-disable no-restricted-syntax */
/* eslint-disable max-len */
/* eslint-disable guard-for-in */

import { AUDIO_DEBUG, AUDIO_PROCESSOR_TYPES } from './audio_constants.ts';
import { AudioProcessorFactory } from './audio_processor.ts';
import { installAudioDebugApi } from './audio_debug_api.ts';

/**
 * Audio Manager class to handle audio processing and UI
 */
export class AudioManager {
  /**
   * Initialize the AudioManager and set up global audio data storage
   */
  constructor(processorType = null) {
    // Auto-select the best processor type if not specified
    if (processorType === null) {
      processorType = AudioProcessorFactory.getBestProcessorType();
      console.log(`🎵 Auto-selected audio processor: ${processorType}`);
      console.log(
        '🎵 (Will automatically fallback to ScriptProcessor if AudioWorklet fails to load)',
      );
    }

    this.processorType = processorType;
    this.initializeGlobalAudioData();
  }

  /**
   * Set up the global audio data storage if it doesn't exist
   */
  initializeGlobalAudioData() {
    if (!window.audioData) {
      console.log('Initializing global audio data storage');
      window.audioData = {
        audioContexts: {}, // Store audio contexts by ID
        audioProcessors: {}, // Store audio processors by ID
        audioSources: {}, // Store MediaElementSourceNodes by ID
        mediaStreams: {}, // Store MediaStreams for microphone capture by ID
        // Note: Audio samples are sent directly to WASM via pushSamplesToWasm()
      };
    }
  }

  /**
   * Set the processor type for new audio setups
   * @param {string} type - Processor type
   */
  setProcessorType(type) {
    if (Object.values(AUDIO_PROCESSOR_TYPES).includes(type)) {
      this.processorType = type;
      console.log(`🎵 Audio processor type set to: ${type}`);
    } else {
      console.warn(`🎵 Invalid processor type: ${type}`);
    }
  }

  /**
   * Get current processor type
   * @returns {string}
   */
  getProcessorType() {
    return this.processorType;
  }

  /**
   * Check if AudioWorklet is supported in this browser
   * @returns {boolean}
   */
  isAudioWorkletSupported() {
    return AudioProcessorFactory.isAudioWorkletSupported();
  }

  /**
   * Get the best available processor type for this browser
   * @returns {string}
   */
  getBestProcessorType() {
    return AudioProcessorFactory.getBestProcessorType();
  }

  /**
   * Switch to AudioWorklet if supported, otherwise fall back to ScriptProcessor
   * @returns {boolean} True if switched to AudioWorklet, false if fell back to ScriptProcessor
   */
  useAudioWorkletIfSupported() {
    if (this.isAudioWorkletSupported()) {
      this.setProcessorType(AUDIO_PROCESSOR_TYPES.AUDIO_WORKLET);
      return true;
    }
    console.warn('🎵 AudioWorklet not supported, using ScriptProcessor');
    this.setProcessorType(AUDIO_PROCESSOR_TYPES.SCRIPT_PROCESSOR);
    return false;
  }

  /**
   * Get processor capabilities and status
   * @returns {Object} Capabilities object
   */
  getCapabilities() {
    return {
      currentProcessor: this.processorType,
      audioWorkletSupported: this.isAudioWorkletSupported(),
      bestAvailable: this.getBestProcessorType(),
      availableTypes: Object.values(AUDIO_PROCESSOR_TYPES),
    };
  }

  /**
   * Set up audio analysis for a given audio element
   * @param {HTMLAudioElement} audioElement - The audio element to analyze
   * @returns {Promise<Object>} Audio analysis components
   */
  async setupAudioAnalysis(audioElement) {
    try {
      // Create and configure the audio context and nodes
      const audioComponents = await this.createAudioComponents(audioElement);

      // Get the audio element's input ID and store references
      const audioId = audioElement.parentNode.querySelector('input').id;
      this.storeAudioReferences(audioId, audioComponents);

      // Start audio processing
      audioComponents.processor.start();

      // Start audio playback
      this.startAudioPlayback(audioElement);

      if (AUDIO_DEBUG.enabled) {
        console.log(
          `🎵 Audio analysis setup complete for ${audioId} using ${audioComponents.processor.getType()}`,
        );
      }

      return audioComponents;
    } catch (error) {
      console.error('🎵 Failed to setup audio analysis:', error);
      throw error;
    }
  }

  /**
   * Create audio context and processing components with automatic fallback
   * @param {HTMLAudioElement} audioElement - The audio element to analyze
   * @returns {Promise<Object>} Created audio components
   */
  async createAudioComponents(audioElement) {
    // Create audio context with browser compatibility
    const AudioContext = window.AudioContext || window.webkitAudioContext;
    const audioContext = new AudioContext();

    if (AUDIO_DEBUG.enabled) {
      console.log(`🎵 Creating new AudioContext (state: ${audioContext.state})`);
    }

    // Create audio source - handle both file-based and stream-based audio
    let source;
    if (audioElement.srcObject && audioElement.srcObject instanceof MediaStream) {
      // For microphone streams, create MediaStreamAudioSourceNode
      source = audioContext.createMediaStreamSource(audioElement.srcObject);
    } else {
      // For file-based audio, create MediaElementAudioSourceNode
      source = audioContext.createMediaElementSource(audioElement);
      source.connect(audioContext.destination); // Connect to output (only for file-based)
    }

    // Create sample callback for the processor
    const sampleCallback = (sampleBuffer, timestamp) => {
      this.handleAudioSamples(sampleBuffer, timestamp, audioElement);
    };

    // Try to create and initialize the preferred processor with fallback
    let processor = null;

    try {
      // First attempt: Try preferred processor type
      processor = AudioProcessorFactory.create(this.processorType, audioContext, sampleCallback);
      await processor.initialize(source);
      console.log(`🎵 Audio processor initialized: ${processor.getType()}`);
    } catch (processorError) {
      console.warn(`🎵 Failed to initialize ${this.processorType}:`, processorError.message);

      // If AudioWorklet failed, try fallback to ScriptProcessor
      if (this.processorType === AUDIO_PROCESSOR_TYPES.AUDIO_WORKLET) {
        try {
          console.log(`🎵 Falling back to ${AUDIO_PROCESSOR_TYPES.SCRIPT_PROCESSOR}`);
          processor = AudioProcessorFactory.create(
            AUDIO_PROCESSOR_TYPES.SCRIPT_PROCESSOR,
            audioContext,
            sampleCallback,
          );
          await processor.initialize(source);
          console.log(`🎵 Successfully using ${processor.getType()} processor`);

          // Update the AudioManager's processor type for future uses
          this.processorType = AUDIO_PROCESSOR_TYPES.SCRIPT_PROCESSOR;
        } catch (fallbackError) {
          console.error('🎵 Both processors failed:', fallbackError);
          throw new Error(
            `Failed to initialize any audio processor. AudioWorklet error: ${processorError.message}, ScriptProcessor error: ${fallbackError.message}`,
          );
        }
      } else {
        // If ScriptProcessor itself failed, re-throw the error
        throw processorError;
      }
    }

    // Handle media element connection errors separately (these aren't processor-specific)
    if (!processor) {
      const error = new Error('No audio processor could be created');
      console.error('🎵 Failed to create audio components:', error);

      // Check for common connection issues
      if (error.name === 'InvalidStateError' && error.message.includes('already connected')) {
        console.error(
          '🎵 The audio element is still connected to a previous MediaElementSourceNode.',
        );
        console.error('🎵 This usually means the cleanup process did not complete properly.');
        console.error('🎵 Try pausing the audio and waiting a moment before switching tracks.');
      }

      throw error;
    }

    console.log(`🎵 Audio components created successfully using ${processor.getType()}`);

    return {
      audioContext,
      source,
      processor,
    };
  }

  /**
   * Handle audio samples from the processor
   * @param {Int16Array} sampleBuffer - Audio samples
   * @param {number} timestamp - Sample timestamp
   * @param {HTMLAudioElement} audioElement - Audio element
   */
  handleAudioSamples(sampleBuffer, timestamp, audioElement) {
    // Push audio samples directly to C++ via WASM (no JS buffering needed)
    // For microphone streams, always push (worklet runs regardless of play state).
    // For file-based audio, only push when playing.
    const isStream = audioElement.srcObject instanceof MediaStream;
    if (isStream || !audioElement.paused) {
      this.updateProcessingIndicator();
      this.pushSamplesToWasm(sampleBuffer, timestamp);
    }
  }

  /**
   * Push audio samples to C++ WASM module via the background worker.
   * AudioManager runs on the main thread but Module lives in the worker,
   * so we send samples via postMessage and the worker calls Module.ccall().
   * @param {Int16Array} sampleBuffer - Audio samples (512 Int16 values)
   * @param {number} timestamp - Sample timestamp in milliseconds
   */
  pushSamplesToWasm(sampleBuffer, timestamp) {
    // Send audio samples to the worker thread where Module is available
    if (typeof window === 'undefined' || !window.fastLEDWorkerManager) {
      if (AUDIO_DEBUG.enabled && Math.random() < AUDIO_DEBUG.sampleRate) {
        console.warn('fastLEDWorkerManager not available - cannot push audio to WASM');
      }
      return;
    }

    const workerManager = window.fastLEDWorkerManager;
    if (!workerManager.isWorkerActive || !workerManager.worker) {
      if (AUDIO_DEBUG.enabled && Math.random() < AUDIO_DEBUG.sampleRate) {
        console.warn('Worker not active - cannot push audio to WASM');
      }
      return;
    }

    try {
      // Copy sampleBuffer into a plain Int16Array for transfer
      const samples = new Int16Array(sampleBuffer);

      // Fire-and-forget: send audio samples to worker via postMessage
      workerManager.worker.postMessage({
        type: 'audio_samples',
        payload: {
          samples: samples.buffer,
          count: samples.length,
          timestamp: timestamp
        }
      }, [samples.buffer]); // Transfer the ArrayBuffer (zero-copy)

      // Debug logging (very sparse to avoid console spam)
      if (AUDIO_DEBUG.enabled && Math.random() < AUDIO_DEBUG.sampleRate * 0.1) {
        console.log(`Pushed ${sampleBuffer.length} samples to worker @ ${timestamp}ms`);
      }
    } catch (error) {
      console.error('Error sending audio samples to worker:', error);
    }
  }

  /**
   * Store audio references in the global audio data object
   * @param {string} audioId - The ID of the audio input
   * @param {Object} components - Audio components to store
   */
  storeAudioReferences(audioId, components) {
    window.audioData.audioContexts[audioId] = components.audioContext;
    window.audioData.audioProcessors[audioId] = components.processor;
    window.audioData.audioSources[audioId] = components.source; // Store source for cleanup
    // Note: Audio samples are sent directly to WASM via pushSamplesToWasm(), no JS buffering needed
  }

  /**
   * Start audio playback and handle errors
   * @param {HTMLAudioElement} audioElement - The audio element to play
   */
  startAudioPlayback(audioElement) {
    audioElement.play().catch((err) => {
      console.error('Error playing audio:', err);
    });
  }

  /**
   * Update the UI to indicate audio is being processed
   */
  updateProcessingIndicator() {
    const label = document.getElementById('canvas-label');
    if (label) {
      label.textContent = 'Audio: Processing';
      if (!label.classList.contains('show-animation')) {
        label.classList.add('show-animation');
      }
    }
  }

  /**
   * Create an audio field UI element
   * @param {Object} element - Element configuration
   * @returns {HTMLElement} The created audio control
   */
  createAudioField(element) {
    // Create the main container and label
    const controlDiv = this.createControlContainer(element);

    // Create file selection components
    const { uploadButton, micButton, audioInput, buttonContainer } = this.createFileSelectionComponents(element);

    // Set up file selection handler
    this.setupFileSelectionHandler(uploadButton, audioInput, controlDiv);

    // Set up microphone capture handler
    this.setupMicrophoneHandler(micButton, controlDiv);

    // Set up drag-and-drop handler
    this.setupDragAndDropHandler(controlDiv, audioInput);

    // Add components to the container
    controlDiv.appendChild(buttonContainer);
    controlDiv.appendChild(audioInput);

    // Auto-load from URL if specified
    if (element.url) {
      this.autoLoadFromUrl(element.url, audioInput, controlDiv);
    }

    return controlDiv;
  }

  /**
   * Auto-load audio from a URL (specified in C++ via UIAudio constructor)
   * @param {string} url - The audio URL to load
   * @param {HTMLInputElement} audioInput - The hidden file input element
   * @param {HTMLElement} controlDiv - The control container
   */
  async autoLoadFromUrl(url, audioInput, controlDiv) {
    try {
      console.log(`🎵 Auto-loading audio from URL: ${url}`);

      // Clean up previous audio context
      await this.cleanupPreviousAudioContext(audioInput.id);

      // Small delay to ensure cleanup is complete
      await new Promise((resolve) => { setTimeout(resolve, 100); });

      // Set up audio playback with fresh audio element
      const audio = this.createOrUpdateAudioElement(controlDiv);

      // Configure and play the audio
      await this.configureAudioPlayback(audio, url, controlDiv);

      // Add processing indicator
      this.updateAudioProcessingIndicator(controlDiv);

      console.log('🎵 Audio auto-load from URL complete');
    } catch (error) {
      console.error('🎵 Error auto-loading audio from URL:', error);
      this.showAudioError(controlDiv, 'Failed to auto-load audio. Click play to retry.');
    }
  }

  /**
   * Create the main control container with label
   * @param {Object} element - Element configuration
   * @returns {HTMLElement} The control container
   */
  createControlContainer(element) {
    const controlDiv = document.createElement('div');
    controlDiv.className = 'ui-control audio-control';

    const labelValueContainer = document.createElement('div');
    labelValueContainer.style.display = 'flex';
    labelValueContainer.style.justifyContent = 'space-between';
    labelValueContainer.style.width = '100%';

    const label = document.createElement('label');
    label.textContent = element.name;
    label.htmlFor = `audio-${element.id}`;

    labelValueContainer.appendChild(label);
    controlDiv.appendChild(labelValueContainer);

    return controlDiv;
  }

  /**
   * Create file selection button and input
   * @param {Object} element - Element configuration
   * @returns {Object} The created components
   */
  createFileSelectionComponents(element) {
    // Create button container for both buttons
    const buttonContainer = document.createElement('div');
    buttonContainer.className = 'audio-button-container';
    buttonContainer.style.display = 'flex';
    buttonContainer.style.gap = '8px';
    buttonContainer.style.marginTop = '5px';

    // Create a custom upload button that matches other UI elements
    const uploadButton = document.createElement('button');
    uploadButton.textContent = '📁 Audio File';
    uploadButton.className = 'audio-upload-button';
    uploadButton.id = `upload-btn-${element.id}`;
    uploadButton.title = 'Select audio file from device';

    // Create microphone button
    const micButton = document.createElement('button');
    micButton.textContent = '🎤 Microphone';
    micButton.className = 'audio-mic-button';
    micButton.id = `mic-btn-${element.id}`;
    micButton.title = 'Capture audio from microphone';

    // Hidden file input
    const audioInput = document.createElement('input');
    audioInput.type = 'file';
    audioInput.id = `audio-${element.id}`;
    audioInput.accept = 'audio/*';
    audioInput.style.display = 'none';

    // Connect button to file input
    uploadButton.addEventListener('click', () => {
      audioInput.click();
    });

    // Add buttons to container
    buttonContainer.appendChild(uploadButton);
    buttonContainer.appendChild(micButton);

    return { uploadButton, micButton, audioInput, buttonContainer };
  }

  /**
   * Set up the file selection handler
   * @param {HTMLButtonElement} uploadButton - The upload button
   * @param {HTMLInputElement} audioInput - The file input
   * @param {HTMLElement} controlDiv - The control container
   */
  setupFileSelectionHandler(uploadButton, audioInput, controlDiv) {
    audioInput.addEventListener('change', async (event) => {
      const file = event.target.files[0];
      if (file) {
        try {
          // Create object URL for the selected file
          const url = URL.createObjectURL(file);

          // Update UI to show selected file
          this.updateButtonText(uploadButton, file);

          // Clean up previous audio context BEFORE setting up new audio
          await this.cleanupPreviousAudioContext(audioInput.id);

          // Small delay to ensure cleanup is complete
          await new Promise((resolve) => { setTimeout(resolve, 100); });

          // Set up audio playback with fresh audio element
          const audio = this.createOrUpdateAudioElement(controlDiv);

          // Configure and play the audio
          await this.configureAudioPlayback(audio, url, controlDiv);

          // Add processing indicator
          this.updateAudioProcessingIndicator(controlDiv);
        } catch (error) {
          console.error('🎵 Error during audio file selection:', error);
          // Show error to user
          this.showAudioError(controlDiv, 'Failed to load audio file. Please try again.');
        }
      }
    });
  }

  /**
   * Set up drag-and-drop handler for audio files on the control container
   * @param {HTMLElement} controlDiv - The control container (drop target)
   * @param {HTMLInputElement} audioInput - The hidden file input element
   */
  setupDragAndDropHandler(controlDiv, audioInput) {
    controlDiv.addEventListener('dragover', (event) => {
      event.preventDefault();
      event.stopPropagation();
      controlDiv.classList.add('audio-drag-over');
    });

    controlDiv.addEventListener('dragleave', (event) => {
      event.preventDefault();
      event.stopPropagation();
      controlDiv.classList.remove('audio-drag-over');
    });

    controlDiv.addEventListener('drop', async (event) => {
      event.preventDefault();
      event.stopPropagation();
      controlDiv.classList.remove('audio-drag-over');

      const files = event.dataTransfer?.files;
      if (!files || files.length === 0) return;

      const file = files[0];
      if (!file.type.startsWith('audio/')) {
        console.warn('Dropped file is not an audio file:', file.type);
        this.showAudioError(controlDiv, 'Please drop an audio file (MP3, WAV, etc.).');
        return;
      }

      console.log(`🎵 Audio file dropped: ${file.name} (${file.type}, ${file.size} bytes)`);

      try {
        const url = URL.createObjectURL(file);

        // Clean up previous audio context
        await this.cleanupPreviousAudioContext(audioInput.id);
        await new Promise((resolve) => { setTimeout(resolve, 100); });

        // Set up audio playback
        const audio = this.createOrUpdateAudioElement(controlDiv);
        await this.configureAudioPlayback(audio, url, controlDiv);
        this.updateAudioProcessingIndicator(controlDiv);

        console.log('🎵 Drag-and-drop audio load complete');
      } catch (error) {
        console.error('🎵 Error loading dropped audio file:', error);
        this.showAudioError(controlDiv, 'Failed to load dropped audio file.');
      }
    });
  }

  /**
   * Set up the microphone capture handler
   * @param {HTMLButtonElement} micButton - The microphone button
   * @param {HTMLElement} controlDiv - The control container
   */
  setupMicrophoneHandler(micButton, controlDiv) {
    let isCapturing = false;

    micButton.addEventListener('click', async () => {
      if (!isCapturing) {
        // Start microphone capture
        try {
          await this.startMicrophoneCapture(micButton, controlDiv);
          isCapturing = true;
        } catch (error) {
          console.error('🎤 Failed to start microphone capture:', error);
          this.showAudioError(controlDiv, 'Failed to access microphone. Please check permissions.');
        }
      } else {
        // Stop microphone capture
        await this.stopMicrophoneCapture(micButton, controlDiv);
        isCapturing = false;
      }
    });
  }

  /**
   * Start microphone capture
   * @param {HTMLButtonElement} micButton - The microphone button
   * @param {HTMLElement} controlDiv - The control container
   */
  async startMicrophoneCapture(micButton, controlDiv) {
    try {
      // Request microphone access
      const stream = await navigator.mediaDevices.getUserMedia({
        audio: {
          echoCancellation: true,
          noiseSuppression: true,
          autoGainControl: true,
          sampleRate: 44100
        }
      });

      // Update button state
      micButton.textContent = '🛑 Stop Recording';
      micButton.className = 'audio-mic-button recording';
      micButton.title = 'Stop microphone recording';

      // Get the audio input ID from the container
      const audioInput = controlDiv.querySelector('input[type="file"]');
      const audioId = audioInput ? audioInput.id : 'unknown';

      // Clean up any previous audio context
      await this.cleanupPreviousAudioContext(audioId);

      // Small delay to ensure cleanup is complete
      await new Promise((resolve) => { setTimeout(resolve, 100); });

      // Create audio element for the stream
      const audio = this.createStreamAudioElement(controlDiv, stream);

      // Set up audio processing for the stream
      await this.setupAudioAnalysis(audio);

      // Update UI to show recording state
      this.updateAudioProcessingIndicator(controlDiv);

      // Store the stream for cleanup
      this.storeMediaStream(audioId, stream);

      console.log('🎤 Microphone capture started successfully');
    } catch (error) {
      console.error('🎤 Error starting microphone capture:', error);
      throw error;
    }
  }

  /**
   * Stop microphone capture
   * @param {HTMLButtonElement} micButton - The microphone button
   * @param {HTMLElement} controlDiv - The control container
   */
  async stopMicrophoneCapture(micButton, controlDiv) {
    try {
      // Get the audio input ID from the container
      const audioInput = controlDiv.querySelector('input[type="file"]');
      const audioId = audioInput ? audioInput.id : 'unknown';

      // Stop the media stream
      const stream = this.getStoredMediaStream(audioId);
      if (stream) {
        stream.getTracks().forEach(track => track.stop());
        this.clearStoredMediaStream(audioId);
      }

      // Clean up audio context
      await this.cleanupPreviousAudioContext(audioId);

      // Remove audio element
      const existingAudio = controlDiv.querySelector('audio');
      if (existingAudio) {
        existingAudio.pause();
        existingAudio.srcObject = null;
        controlDiv.removeChild(existingAudio);
      }

      // Reset button state
      micButton.textContent = '🎤 Microphone';
      micButton.className = 'audio-mic-button';
      micButton.title = 'Capture audio from microphone';

      // Remove processing indicator
      const existingIndicator = controlDiv.querySelector('.audio-indicator');
      if (existingIndicator) {
        controlDiv.removeChild(existingIndicator);
      }

      console.log('🎤 Microphone capture stopped');
    } catch (error) {
      console.error('🎤 Error stopping microphone capture:', error);
    }
  }

  /**
   * Create an audio element for the media stream
   * @param {HTMLElement} container - The control container
   * @param {MediaStream} stream - The media stream
   * @returns {HTMLAudioElement} The audio element
   */
  createStreamAudioElement(container, stream) {
    // Remove any existing audio element first
    const existingAudio = container.querySelector('audio');
    if (existingAudio) {
      existingAudio.pause();
      existingAudio.srcObject = null;
      container.removeChild(existingAudio);
    }

    // Create new audio element for the stream
    const audio = document.createElement('audio');
    audio.controls = false; // Hide controls for microphone stream
    audio.muted = true; // Mute to prevent feedback
    audio.className = 'audio-player stream';
    audio.srcObject = stream;

    // Get the audio input ID from the container
    const audioInput = container.querySelector('input[type="file"]');
    const audioId = audioInput ? audioInput.id : 'unknown';
    audio.setAttribute('data-audio-id', audioId);

    container.appendChild(audio);

    // Start playing the stream (muted)
    audio.play().catch(err => {
      console.warn('🎤 Could not auto-play stream (this is normal):', err);
    });

    return audio;
  }

  /**
   * Store a media stream for later cleanup
   * @param {string} audioId - The audio ID
   * @param {MediaStream} stream - The media stream
   */
  storeMediaStream(audioId, stream) {
    if (!window.audioData.mediaStreams) {
      window.audioData.mediaStreams = {};
    }
    window.audioData.mediaStreams[audioId] = stream;
  }

  /**
   * Get a stored media stream
   * @param {string} audioId - The audio ID
   * @returns {MediaStream|null} The stored stream or null
   */
  getStoredMediaStream(audioId) {
    return window.audioData.mediaStreams?.[audioId] || null;
  }

  /**
   * Clear a stored media stream
   * @param {string} audioId - The audio ID
   */
  clearStoredMediaStream(audioId) {
    if (window.audioData.mediaStreams?.[audioId]) {
      delete window.audioData.mediaStreams[audioId];
    }
  }

  /**
   * Update button text to show selected file name
   * @param {HTMLButtonElement} button - The upload button
   * @param {File} file - The selected audio file
   */
  updateButtonText(button, file) {
    button.textContent = file.name.length > 20 ? `${file.name.substring(0, 17)}...` : file.name;
  }

  /**
   * Create or update the audio element
   * @param {HTMLElement} container - The control container
   * @returns {HTMLAudioElement} The audio element
   */
  createOrUpdateAudioElement(container) {
    // Get the audio input ID from the container
    const audioInput = container.querySelector('input[type="file"]');
    const audioId = audioInput ? audioInput.id : 'unknown';

    // Remove any existing audio element first
    const existingAudio = container.querySelector('audio');
    if (existingAudio) {
      existingAudio.pause();
      existingAudio.currentTime = 0;
      existingAudio.src = '';
      existingAudio.load();
      existingAudio.remove();
    }

    // Remove any existing custom player
    const existingCustomPlayer = container.querySelector('.custom-audio-player');
    if (existingCustomPlayer) {
      existingCustomPlayer.remove();
    }

    // Create audio element (hidden, used for playback)
    const audio = document.createElement('audio');
    audio.controls = false;
    audio.className = 'audio-player';
    audio.setAttribute('data-audio-id', audioId);
    container.appendChild(audio);

    // Create custom player UI
    this.createCustomAudioPlayer(container, audio, audioId);

    return audio;
  }

  /**
   * Create custom audio player UI with full control styling
   * @param {HTMLElement} container - The container element
   * @param {HTMLAudioElement} audio - The audio element
   * @param {string} audioId - The audio input ID
   */
  createCustomAudioPlayer(container, audio, audioId) {
    // Create custom player wrapper
    const customPlayer = document.createElement('div');
    customPlayer.className = 'custom-audio-player';
    customPlayer.setAttribute('data-audio-id', audioId);

    // Create controls row
    const controlsRow = document.createElement('div');
    controlsRow.className = 'custom-audio-controls';

    // Play/Pause button
    const playBtn = document.createElement('button');
    playBtn.className = 'audio-play-btn';
    playBtn.innerHTML = `
      <svg class="play-icon" viewBox="0 0 24 24">
        <path d="M8 5v14l11-7z"/>
      </svg>
      <svg class="pause-icon" style="display:none;" viewBox="0 0 24 24">
        <path d="M6 4h4v16H6V4zm8 0h4v16h-4V4z"/>
      </svg>
    `;

    // Time display
    const timeDisplay = document.createElement('div');
    timeDisplay.className = 'audio-time';
    timeDisplay.textContent = '0:00 / 0:00';

    // Volume control
    const volumeControl = document.createElement('div');
    volumeControl.className = 'audio-volume-control';

    const volumeBtn = document.createElement('button');
    volumeBtn.className = 'audio-volume-btn';
    volumeBtn.innerHTML = `
      <svg viewBox="0 0 24 24">
        <path d="M3 9v6h4l5 5V4L7 9H3zm13.5 3c0-1.77-1.02-3.29-2.5-4.03v8.05c1.48-.73 2.5-2.25 2.5-4.02z"/>
      </svg>
    `;

    const volumeSlider = document.createElement('input');
    volumeSlider.type = 'range';
    volumeSlider.className = 'audio-volume-slider';
    volumeSlider.min = '0';
    volumeSlider.max = '100';
    volumeSlider.value = '100';

    volumeControl.appendChild(volumeBtn);
    volumeControl.appendChild(volumeSlider);

    // Assemble controls row
    controlsRow.appendChild(playBtn);
    controlsRow.appendChild(timeDisplay);
    controlsRow.appendChild(volumeControl);

    // Progress bar container
    const progressContainer = document.createElement('div');
    progressContainer.className = 'audio-progress-container';

    const progressBg = document.createElement('div');
    progressBg.className = 'audio-progress-bg';

    const progressFill = document.createElement('div');
    progressFill.className = 'audio-progress-fill';

    const scrubberHandle = document.createElement('div');
    scrubberHandle.className = 'audio-scrubber-handle';

    progressFill.appendChild(scrubberHandle);
    progressBg.appendChild(progressFill);
    progressContainer.appendChild(progressBg);

    // Assemble custom player
    customPlayer.appendChild(controlsRow);
    customPlayer.appendChild(progressContainer);
    container.appendChild(customPlayer);

    // Wire up event handlers
    this.setupCustomPlayerEvents(audio, playBtn, timeDisplay, progressContainer, progressFill, volumeSlider, volumeBtn);
  }

  /**
   * Set up event handlers for custom audio player
   */
  setupCustomPlayerEvents(audio, playBtn, timeDisplay, progressContainer, progressFill, volumeSlider, volumeBtn) {
    const playIcon = playBtn.querySelector('.play-icon');
    const pauseIcon = playBtn.querySelector('.pause-icon');

    // Play/Pause toggle
    playBtn.addEventListener('click', () => {
      if (audio.paused) {
        audio.play();
        playIcon.style.display = 'none';
        pauseIcon.style.display = 'block';
      } else {
        audio.pause();
        playIcon.style.display = 'block';
        pauseIcon.style.display = 'none';
      }
    });

    // Update time display and progress
    audio.addEventListener('timeupdate', () => {
      const currentTime = this.formatTime(audio.currentTime);
      const duration = this.formatTime(audio.duration);
      timeDisplay.textContent = `${currentTime} / ${duration}`;

      // Update progress bar
      const progress = (audio.currentTime / audio.duration) * 100 || 0;
      progressFill.style.width = `${progress}%`;
    });

    // Reset play button on audio end
    audio.addEventListener('ended', () => {
      playIcon.style.display = 'block';
      pauseIcon.style.display = 'none';
    });

    // Seek on progress bar click - with proper cleanup
    let isSeeking = false;

    const seek = (e) => {
      const rect = progressContainer.getBoundingClientRect();
      const x = e.clientX - rect.left;
      const percentage = Math.max(0, Math.min(1, x / rect.width));
      audio.currentTime = percentage * audio.duration;
    };

    const handleMouseMove = (e) => {
      if (isSeeking) {
        seek(e);
      }
    };

    const handleMouseUp = () => {
      isSeeking = false;
      // Clean up document-level listeners when seeking ends
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);
    };

    progressContainer.addEventListener('mousedown', (e) => {
      isSeeking = true;
      seek(e);
      // Add document-level listeners only when actively seeking
      document.addEventListener('mousemove', handleMouseMove);
      document.addEventListener('mouseup', handleMouseUp);
    });

    progressContainer.addEventListener('click', seek);

    // Volume control
    volumeSlider.addEventListener('input', (e) => {
      audio.volume = e.target.value / 100;
    });

    volumeBtn.addEventListener('click', () => {
      if (audio.volume > 0) {
        audio.volume = 0;
        volumeSlider.value = 0;
      } else {
        audio.volume = 1;
        volumeSlider.value = 100;
      }
    });
  }

  /**
   * Format time in seconds to MM:SS
   */
  formatTime(seconds) {
    if (isNaN(seconds) || seconds === Infinity) {
      return '0:00';
    }
    const mins = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    return `${mins}:${secs.toString().padStart(2, '0')}`;
  }

  /**
   * Clean up any previous audio context and buffer storage
   * @param {string} inputId - The ID of the audio input
   * @returns {Promise<void>}
   */
  async cleanupPreviousAudioContext(inputId) {
    if (AUDIO_DEBUG.enabled) {
      console.log(`🎵 Starting cleanup for ${inputId}`);
    }

    // Clean up audio processor first (this disconnects nodes)
    if (window.audioData?.audioProcessors?.[inputId]) {
      const processor = window.audioData.audioProcessors[inputId];
      if (AUDIO_DEBUG.enabled) {
        console.log(`🎵 Cleaning up ${processor.getType()} processor for ${inputId}`);
      }
      processor.cleanup();
      delete window.audioData.audioProcessors[inputId];
    }

    // Clean up MediaElementSourceNode
    if (window.audioData?.audioSources?.[inputId]) {
      try {
        const source = window.audioData.audioSources[inputId];
        source.disconnect();
      } catch (e) {
        console.warn('Error disconnecting MediaElementSourceNode:', e);
      }
      delete window.audioData.audioSources[inputId];
    }

    // Clean up audio context and wait for it to close
    if (window.audioData?.audioContexts?.[inputId]) {
      try {
        const context = window.audioData.audioContexts[inputId];
        await context.close();
      } catch (e) {
        console.warn('Error closing previous audio context:', e);
      }
      delete window.audioData.audioContexts[inputId];
    }

    // Clean up media streams
    if (window.audioData?.mediaStreams?.[inputId]) {
      const stream = window.audioData.mediaStreams[inputId];
      stream.getTracks().forEach(track => track.stop());
      delete window.audioData.mediaStreams[inputId];
    }

    // Clean up any lingering audio elements in the DOM that might be associated with this ID
    const audioElements = document.querySelectorAll(
      `#audio-${inputId}, audio[data-audio-id="${inputId}"]`,
    );
    audioElements.forEach(/** @param {HTMLAudioElement} audio */(audio) => {
      audio.pause();
      audio.src = '';
      audio.load();
    });
  }

  /**
   * Configure and play the audio
   * @param {HTMLAudioElement} audio - The audio element
   * @param {string} url - The audio file URL
   * @param {HTMLElement} container - The control container
   */
  async configureAudioPlayback(audio, url, container) {
    // Set source and loop
    audio.src = url;
    audio.loop = true;

    try {
      // Initialize audio analysis before playing
      await this.setupAudioAnalysis(audio);

      // Try to play the audio (may be blocked by browser policies)
      await audio.play();
    } catch (err) {
      console.error('🎵 Error during audio playback setup:', err);
      this.createFallbackPlayButton(audio, container);
      throw err; // Re-throw so the caller can handle it
    }
  }

  /**
   * Create a fallback play button when autoplay is blocked
   * @param {HTMLAudioElement} audio - The audio element
   * @param {HTMLElement} container - The control container
   */
  createFallbackPlayButton(audio, container) {
    const playButton = document.createElement('button');
    playButton.textContent = 'Play Audio';
    playButton.className = 'audio-play-button';
    playButton.onclick = () => {
      audio.play();
    };
    container.appendChild(playButton);
  }

  /**
   * Update the audio processing indicator
   * @param {HTMLElement} container - The control container
   */
  updateAudioProcessingIndicator(container) {
    // Create new indicator
    const audioIndicator = document.createElement('div');
    audioIndicator.className = 'audio-indicator';
    audioIndicator.textContent = 'Audio samples ready';

    // Replace any existing indicator (including errors)
    const existingIndicator = container.querySelector('.audio-indicator, .audio-error');
    if (existingIndicator) {
      container.removeChild(existingIndicator);
    }

    container.appendChild(audioIndicator);
  }

  /**
   * Show an error message in the audio control container
   * @param {HTMLElement} container - The control container
   * @param {string} message - Error message to display
   */
  showAudioError(container, message) {
    // Create error indicator
    const errorIndicator = document.createElement('div');
    errorIndicator.className = 'audio-error';
    errorIndicator.textContent = message;
    errorIndicator.style.color = 'red';
    errorIndicator.style.fontSize = '12px';
    errorIndicator.style.marginTop = '5px';

    // Replace any existing indicator
    const existingIndicator = container.querySelector('.audio-indicator, .audio-error');
    if (existingIndicator) {
      container.removeChild(existingIndicator);
    }

    container.appendChild(errorIndicator);
  }
}

/**
 * Create a global instance of AudioManager
 */
const audioManager = new AudioManager();

// Install the `window.*` debug API surface on module load.
installAudioDebugApi(audioManager);

// Note: AudioBufferStorage class removed - audio samples are sent directly to WASM
// via pushSamplesToWasm() and stored in C++ ring buffer. No JS-side buffering needed.
