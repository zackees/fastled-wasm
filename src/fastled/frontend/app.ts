// @ts-nocheck
/**
 * FastLED WASM Application Entry Point
 *
 * This file was extracted from inline <script> blocks in index.html
 * so that Vite can process and bundle it properly.
 */

// Three.js imports are vendored locally so the frontend can build without npm.
import * as THREE from './vendor/three/build/three.module.js';
import Stats from './vendor/three/examples/jsm/libs/stats.module.js';
import { GUI } from './vendor/three/examples/jsm/libs/lil-gui.module.min.js';
import { OrbitControls } from './vendor/three/examples/jsm/controls/OrbitControls.js';
import { GLTFLoader } from './vendor/three/examples/jsm/loaders/GLTFLoader.js';
import { EffectComposer } from './vendor/three/examples/jsm/postprocessing/EffectComposer.js';
import { RenderPass } from './vendor/three/examples/jsm/postprocessing/RenderPass.js';
import { UnrealBloomPass } from './vendor/three/examples/jsm/postprocessing/UnrealBloomPass.js';
import { OutputPass } from './vendor/three/examples/jsm/postprocessing/OutputPass.js';
import * as BufferGeometryUtils from './vendor/three/examples/jsm/utils/BufferGeometryUtils.js';

// Make Three.js modules available globally for the worker and other modules
window.threeJsModules = {
    THREE, Stats, GUI, OrbitControls, GLTFLoader,
    EffectComposer, RenderPass, UnrealBloomPass, OutputPass, BufferGeometryUtils,
};

// FastLED main module
import { loadFastLED } from './index.ts';

// Unified canvas UI fade-in/fade-out logic
let canvasUITimeout = null;
const canvas = document.getElementById('myCanvas');
const recordButton = document.getElementById('record-btn');
const canvasLabel = document.getElementById('canvas-label');

function showCanvasUI() {
    if (canvasUITimeout) {
        clearTimeout(canvasUITimeout);
        canvasUITimeout = null;
    }
    if (recordButton) recordButton.classList.add('visible');
    if (canvasLabel) canvasLabel.classList.add('visible');
}

function hideCanvasUIWithDelay() {
    if (canvasUITimeout) {
        clearTimeout(canvasUITimeout);
    }
    canvasUITimeout = setTimeout(() => {
        if (recordButton) recordButton.classList.remove('visible');
        if (canvasLabel) canvasLabel.classList.remove('visible');
        canvasUITimeout = null;
    }, 2000); // 2 second delay
}

// Add event listeners for canvas hover - controls both record button and label
if (canvas) {
    canvas.addEventListener('mouseenter', showCanvasUI);
    canvas.addEventListener('mouseleave', hideCanvasUIWithDelay);
}

// Also handle record button hover to keep UI visible
if (recordButton) {
    recordButton.addEventListener('mouseenter', showCanvasUI);
    recordButton.addEventListener('mouseleave', hideCanvasUIWithDelay);
}

// Handle canvas label hover to keep UI visible
if (canvasLabel) {
    canvasLabel.addEventListener('mouseenter', showCanvasUI);
    canvasLabel.addEventListener('mouseleave', hideCanvasUIWithDelay);
}

const myCanvas = document.getElementById('myCanvas');
if (myCanvas) {
    myCanvas.addEventListener('click', function() {
        const output = document.getElementById('output');
        const popup = document.getElementById('output-popup');
        const overlay = document.getElementById('popup-overlay');
        if (!output || !popup || !overlay) return;

        // Move the output element to the popup
        popup.appendChild(output);

        // Show the popup and overlay with animation
        overlay.style.opacity = '0';
        overlay.style.display = 'block';
        popup.style.display = 'block';

        // Trigger reflow
        void overlay.offsetWidth;

        // Fade in
        overlay.style.opacity = '1';

        // Close popup when clicking overlay
        overlay.onclick = function() {
            // Fade out
            overlay.style.opacity = '0';

            setTimeout(() => {
                popup.style.display = 'none';
                overlay.style.display = 'none';

                // Reset the canvas UI fade state when popup closes
                const label = document.getElementById('canvas-label');
                if (label) {
                    label.classList.remove('visible');
                    // Force reflow to ensure state resets properly
                    void label.offsetWidth;
                }
            }, 200); // Match the transition duration
        };
    });
}

const outputEl = document.getElementById('output');
if (outputEl) outputEl.addEventListener('click', function() {
    if (document.querySelector('.toast-notification')) {
        return; // We are already showing a toast notification
    }

    const text = this.textContent;

    navigator.clipboard.writeText(text).then(() => {
        const toast = document.createElement('div');
        toast.className = 'toast-notification';
        toast.textContent = 'Copied to clipboard';
        document.body.appendChild(toast);

        // Force reflow
        void toast.offsetHeight;

        toast.classList.add('toast-notification-show');

        setTimeout(() => {
            toast.classList.add('toast-notification-hide');
            toast.addEventListener('transitionend', () => {
                if (toast.parentElement) {
                    document.body.removeChild(toast);
                }
            }, {once: true});
        }, 2000);
    }).catch(err => {
        console.error('Failed to copy text: ', err);
    });
});

// Handle File menu items
const fileMenuItems = document.querySelectorAll('.menu-item:first-child .menu-dropdown-item');
fileMenuItems.forEach(item => {
    item.addEventListener('click', function(e) {
        try {
            e.stopPropagation();
            const action = this.getAttribute('data-action');

            if (action) {
                console.log('File action selected:', action);
                handleFileAction(action);
            } else {
                console.warn('File menu item missing data-action attribute:', this);
            }
        } catch (error) {
            console.error('Error handling File menu click:', error);
            throw error;
        }
    });
});

// Handle Settings menu items
const settingsMenuItems = document.querySelectorAll('.menu-item:nth-child(2) .menu-dropdown-item');
settingsMenuItems.forEach(item => {
    item.addEventListener('click', function(e) {
        try {
            e.stopPropagation();
            const selectedValue = this.getAttribute('data-value');
            const hasSubmenu = this.classList.contains('has-submenu');

            // If this is a submenu parent, don't trigger navigation
            if (hasSubmenu) {
                return;
            }

            if (selectedValue) {
                console.log('Settings option selected:', selectedValue);
                handleGraphicsOptionChange(selectedValue);
            } else {
                console.warn('Settings menu item missing data-value attribute:', this);
            }
        } catch (error) {
            console.error('Error handling Settings menu click:', error);
            throw error;
        }
    });
});

// Handle Settings submenu items (gfx mode options)
const submenuItems = document.querySelectorAll('.menu-item:nth-child(2) .submenu-dropdown .menu-dropdown-item');
submenuItems.forEach(item => {
    item.addEventListener('click', function(e) {
        try {
            e.stopPropagation();
            const selectedValue = this.getAttribute('data-value');

            if (selectedValue) {
                console.log('Settings submenu option selected:', selectedValue);
                handleGraphicsOptionChange(selectedValue);
            } else {
                console.warn('Settings submenu item missing data-value attribute:', this);
            }
        } catch (error) {
            console.error('Error handling Settings submenu click:', error);
            throw error;
        }
    });
});

// Handle Tools menu items
const toolsMenuItems = document.querySelectorAll('.menu-item:nth-child(3) .menu-dropdown-item');
toolsMenuItems.forEach(item => {
    item.addEventListener('click', function(e) {
        try {
            e.stopPropagation();
            const selectedValue = this.getAttribute('data-value');
            const selectedAction = this.getAttribute('data-action');

            if (selectedValue) {
                console.log('Tools option selected:', selectedValue);
                handleToolsOptionChange(selectedValue);
            } else if (selectedAction) {
                console.log('Tools action selected:', selectedAction);
                handleToolsActionChange(selectedAction);
            } else {
                console.warn('Tools menu item missing data-value or data-action attribute:', this);
            }
        } catch (error) {
            console.error('Error handling Tools menu click:', error);
            throw error;
        }
    });
});

// Handle file actions
function handleFileAction(action) {
    if (action === 'save') {
        console.log('Save action triggered - forwarding to /save endpoint');
        // Forward to /save endpoint on the server
        window.location.href = '/save';
    }
}

// Handle tools option changes
function handleToolsOptionChange(option) {
    if (option === 'ui-events') {
        // Handle UI Events (JSON Inspector) without page reload
        if (window.jsonInspector) {
            window.jsonInspector.toggle();
        } else {
            console.warn('JSON Inspector not available yet');
        }
        return;
    }
}

// Handle tools action changes
function handleToolsActionChange(action) {
    if (action === 'video-mapper') {
        // Open FastLED Video Mapper in new tab
        window.open('https://ledmapper.com', '_blank');
        return;
    }
}

// Handle graphics option changes
function handleGraphicsOptionChange(option) {
    if (option === 'video-settings') {
        // Show video settings popup
        showVideoSettingsPopup();
        return;
    }

    if (option === 'toggle-auto-bloom') {
        // Don't process as regular option - handled by checkbox
        return;
    }

    // Determine URL parameter based on selection
    let gfxParam = '';
    if (option === 'gfx-simple') {
        gfxParam = '0'; // Force fast renderer
    } else if (option === 'gfx-bloom') {
        gfxParam = '1'; // Force ThreeJS renderer with bloom
    }

    // Update URL and reload page with new graphics setting
    const url = new URL(window.location);
    if (gfxParam) {
        url.searchParams.set('gfx', gfxParam);
    } else {
        url.searchParams.delete('gfx');
    }

    // Show a brief notification before reloading
    const toast = document.createElement('div');
    toast.className = 'toast-notification';
    toast.textContent = `Switching to ${option.replace('-', ' ')}...`;
    document.body.appendChild(toast);

    // Force reflow
    void toast.offsetHeight;
    toast.classList.add('toast-notification-show');

    // Reload with new graphics setting after brief delay
    setTimeout(() => {
        window.location.href = url.toString();
    }, 500);
}

// Video Settings Popup Functions - Optimized for encoding speed with MP4 compatibility
const videoSettingsDefaults = {
    videoCodec: 'video/mp4;codecs=avc1.42E01E',  // H.264 Baseline for fastest encoding with MP4
    videoBitrate: 8,                             // Slightly lower bitrate for speed
    audioCodec: 'aac',                           // AAC for MP4 compatibility
    audioBitrate: 128,
    fps: 60
};

let videoSettings = { ...videoSettingsDefaults };

// Always use default settings (no localStorage persistence)
function loadVideoSettings() {
    // Aggressive cleanup of any video-related localStorage keys
    const keysToRemove = [
        'videoRecordingSettings',
        'videoSettings',
        'fastled_video_settings',
        'recording_settings'
    ];

    keysToRemove.forEach(key => {
        if (localStorage.getItem(key)) {
            localStorage.removeItem(key);
            console.log(`Removed old ${key} from localStorage`);
        }
    });

    // Always return fresh defaults - no persistence needed
    videoSettings = { ...videoSettingsDefaults };
    console.log('Video settings reset to defaults:', videoSettings);
    return videoSettings;
}

// No longer saving to localStorage - always use defaults
function saveVideoSettings() {
    // Settings are not persisted - always use defaults on reload
    console.log('Video settings not saved (using defaults only)');
}

// Helper to safely set an input element's value
function setInputValue(id, value) {
    const el = document.getElementById(id);
    if (el) el.value = value;
}

// Show video settings popup
function showVideoSettingsPopup() {
    const overlay = document.getElementById('video-settings-overlay');
    const popup = document.getElementById('video-settings-popup');
    if (!overlay || !popup) return;

    // Load current settings
    loadVideoSettings();

    // Set form values
    setInputValue('video-codec', videoSettings.videoCodec);
    setInputValue('video-bitrate', videoSettings.videoBitrate);
    setInputValue('audio-codec', videoSettings.audioCodec);
    setInputValue('audio-bitrate', videoSettings.audioBitrate);
    setInputValue('video-fps', videoSettings.fps);

    // Update bitrate displays
    updateBitrateDisplays();

    // Show popup
    overlay.style.display = 'block';
    popup.style.display = 'block';
    overlay.style.opacity = '0';
    void overlay.offsetWidth;
    overlay.style.opacity = '1';
}

// Hide video settings popup
function hideVideoSettingsPopup() {
    const overlay = document.getElementById('video-settings-overlay');
    const popup = document.getElementById('video-settings-popup');
    if (!overlay || !popup) return;

    overlay.style.opacity = '0';
    setTimeout(() => {
        overlay.style.display = 'none';
        popup.style.display = 'none';
    }, 200);
}

// Update bitrate display labels
function updateBitrateDisplays() {
    const videoBitrate = document.getElementById('video-bitrate');
    const audioBitrate = document.getElementById('audio-bitrate');

    if (videoBitrate && videoBitrate.nextElementSibling) {
        videoBitrate.nextElementSibling.textContent = `${videoBitrate.value} Mbps`;
    }
    if (audioBitrate && audioBitrate.nextElementSibling) {
        audioBitrate.nextElementSibling.textContent = `${audioBitrate.value} kbps`;
    }
}

// Helper to add event listener with null check
function addListener(id, event, handler) {
    const el = document.getElementById(id);
    if (el) el.addEventListener(event, handler);
}

// Setup video settings event handlers
addListener('video-bitrate', 'input', updateBitrateDisplays);
addListener('audio-bitrate', 'input', updateBitrateDisplays);

addListener('video-settings-apply', 'click', () => {
    // Collect settings from form
    const getVal = (id) => {
        const el = document.getElementById(id);
        return el ? el.value : '';
    };
    videoSettings.videoCodec = getVal('video-codec');
    videoSettings.videoBitrate = parseFloat(getVal('video-bitrate')) || videoSettingsDefaults.videoBitrate;
    videoSettings.audioCodec = getVal('audio-codec');
    videoSettings.audioBitrate = parseInt(getVal('audio-bitrate'), 10) || videoSettingsDefaults.audioBitrate;
    videoSettings.fps = parseInt(getVal('video-fps'), 10) || videoSettingsDefaults.fps;

    // Save settings
    saveVideoSettings();

    // Apply to video recorder if it exists
    if (window.videoRecorder) {
        window.videoRecorder.updateSettings(videoSettings);

        // Update record button tooltip with new encoding format
        if (window.updateRecordButtonTooltip) {
            window.updateRecordButtonTooltip();
        }
    }

    // Show confirmation toast
    const toast = document.createElement('div');
    toast.className = 'toast-notification';
    toast.textContent = 'Video settings saved';
    document.body.appendChild(toast);
    void toast.offsetHeight;
    toast.classList.add('toast-notification-show');
    setTimeout(() => {
        toast.classList.add('toast-notification-hide');
        toast.addEventListener('transitionend', () => {
            if (toast.parentElement) {
                document.body.removeChild(toast);
            }
        }, {once: true});
    }, 2000);

    hideVideoSettingsPopup();
});

addListener('video-settings-cancel', 'click', hideVideoSettingsPopup);

addListener('video-settings-reset', 'click', () => {
    // Reset to defaults
    setInputValue('video-codec', videoSettingsDefaults.videoCodec);
    setInputValue('video-bitrate', videoSettingsDefaults.videoBitrate);
    setInputValue('audio-codec', videoSettingsDefaults.audioCodec);
    setInputValue('audio-bitrate', videoSettingsDefaults.audioBitrate);
    setInputValue('video-fps', videoSettingsDefaults.fps);
    updateBitrateDisplays();
});

addListener('video-settings-overlay', 'click', hideVideoSettingsPopup);

// Initialize video settings on page load to ensure defaults are set
loadVideoSettings();

// Set form values immediately to override HTML defaults
setInputValue('video-codec', videoSettings.videoCodec);
setInputValue('video-bitrate', videoSettings.videoBitrate);
setInputValue('audio-codec', videoSettings.audioCodec);
setInputValue('audio-bitrate', videoSettings.audioBitrate);
setInputValue('video-fps', videoSettings.fps);

// Expose functions globally
window.showVideoSettingsPopup = showVideoSettingsPopup;
window.getVideoSettings = loadVideoSettings;

// Auto-bloom toggle handler
const autoBloomToggle = document.getElementById('auto-bloom-toggle');
if (autoBloomToggle) {
    autoBloomToggle.addEventListener('change', function(e) {
        e.stopPropagation(); // Prevent menu from closing
        if (window.graphicsManager) {
            window.graphicsManager.auto_bloom_enabled = this.checked;
            console.log('Auto-bloom', this.checked ? 'enabled' : 'disabled');
        }
    });

    // Prevent clicking the label from closing the menu
    const autoBloomLabel = autoBloomToggle.parentElement;
    if (autoBloomLabel) {
        autoBloomLabel.addEventListener('click', function(e) {
            e.stopPropagation();
        });
    }
}

async function loadThreeJs() {
    console.log('Using ThreeJS modules:', window.threeJsModules);
    return {
        containerId: "container",
        modules: window.threeJsModules,
    };
}

// Dynamically load a non-module script and wait for it to complete
function loadScript(src: string): Promise<void> {
    return new Promise((resolve, reject) => {
        const script = document.createElement('script');
        script.src = src;
        script.onload = () => resolve();
        script.onerror = () => reject(new Error(`Failed to load script: ${src}`));
        document.head.appendChild(script);
    });
}

// load fastled when the window is loaded.
async function runFastLED() {
    // Load Emscripten-generated fastled.js and Three.js modules in parallel
    const [, threeJs] = await Promise.all([loadScript('fastled.js'), loadThreeJs()]);
    const options = {
        canvasId: "myCanvas",
        uiControlsId: "ui-controls",
        printId: "output",
        frameRate: 60,
        fastled: fastled,
        threeJs: threeJs,
    };
    await loadFastLED(options);

    // Show the canvas UI briefly on initial load, then hide automatically
    setTimeout(() => {
        showCanvasUI();
        hideCanvasUIWithDelay();
    }, 500); // Brief delay after page load
}

// Wait for fonts to load before showing content
if (document.fonts && document.fonts.ready) {
    document.fonts.ready.then(function () {
        document.body.style.opacity = 1;
    });
} else {
    // Fallback for browsers that do not support document.fonts
    window.onload = function () {
        document.body.style.opacity = 1;
    };
}

let loaded = false;
const loadFastLedOnce = () => {
    if (loaded) return;
    runFastLED();
    loaded = true;
};

window.addEventListener('load', loadFastLedOnce);
setTimeout(loadFastLedOnce, 100);
