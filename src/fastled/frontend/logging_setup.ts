// @ts-nocheck
/// <reference path="types.d.ts" />

/**
 * Logging setup for the FastLED frontend.
 *
 * Provides timestamped console replacements and a custom print sink that
 * appends to the configured output element. The console override must be
 * installed before any other module logs (other than the very first lines
 * in `index.ts`), so callers should invoke `installConsoleOverride()` early
 * in the index module load sequence.
 */

import { state, MAX_STDOUT_LINES } from './state.ts';

/** Application start time epoch for timing calculations */
const EPOCH = new Date().getTime();

/**
 * Gets elapsed time since application start
 * @returns {string} Time in seconds with one decimal place
 */
export function getTimeSinceEpoc() {
  const outMS = new Date().getTime() - EPOCH;
  const outSec = outMS / 1000;
  // one decimal place
  return outSec.toFixed(1);
}

/** Store reference to original console for fallback */
const prev_console = console;

// Store original console methods
const _prev_log = prev_console.log;
const _prev_warn = prev_console.warn;
const _prev_error = prev_console.error;

/**
 * Adds timestamp to console arguments
 * @param {...*} args - Console arguments to timestamp
 * @returns {Array} Arguments array with timestamp prepended
 */
export function toStringWithTimeStamp(...args) {
  const time = `${getTimeSinceEpoc()}s`;
  return [time, ...args]; // Return array with time prepended, don't join
}

/**
 * Custom console.log implementation with timestamps
 * @param {...*} args - Arguments to log
 */
export function log(...args) {
  const argsWithTime = toStringWithTimeStamp(...args);
  _prev_log(...argsWithTime); // Spread the array when calling original logger
  try {
    state.print(...argsWithTime);
  } catch (e) {
    _prev_log('Error in log', e);
  }
}

/**
 * Custom console.warn implementation with timestamps
 * @param {...*} args - Arguments to warn about
 */
export function warn(...args) {
  const argsWithTime = toStringWithTimeStamp(...args);
  _prev_warn(...argsWithTime);
  try {
    state.print(...argsWithTime);
  } catch (e) {
    _prev_warn('Error in warn', e);
  }
}

/**
 * Custom print function for displaying output in the UI
 * @param {...*} args - Arguments to print to UI output
 */
export function customPrintFunction(...args) {
  if (state.containerId === undefined) {
    return; // Not ready yet.
  }
  // take the args and stringify them, then add them to the output element
  const cleanedArgs = args.map((arg) => {
    if (typeof arg === 'object') {
      try {
        return JSON.stringify(arg).slice(0, 100);
      } catch (e) {
        return `${arg}`;
      }
    }
    return arg;
  });

  const output = document.getElementById(state.outputId);
  const allText = `${output.textContent + [...cleanedArgs].join(' ')}\n`;
  // split into lines, and if there are more than 100 lines, remove one.
  const lines = allText.split('\n');
  while (lines.length > MAX_STDOUT_LINES) {
    lines.shift();
  }
  output.textContent = lines.join('\n');
}

/**
 * Installs the timestamped console overrides.
 *
 * DO NOT OVERRIDE ERROR! When something goes really wrong we want it
 * to always go to the console. If we hijack it then startup errors become
 * extremely difficult to debug.
 *
 * Note: Modifying existing console properties instead of reassigning the global.
 */
export function installConsoleOverride() {
  console.log = log;
  console.warn = warn;
  console.error = _prev_error;
}
