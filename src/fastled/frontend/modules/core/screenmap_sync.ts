// @ts-nocheck
/// <reference path="../../types.d.ts" />

/**
 * @fileoverview Screen map synchronization helpers.
 *
 * This module answers a single question: "which strips in a frame have no
 * cached screenmap layout yet?" FastLED creates default screenmap layouts
 * lazily on the C++ side (see jsFillInMissingScreenMaps), which runs only on
 * the first exported frame. Until that first frame has been processed and
 * its resulting screenmap has been synced into the worker/main-thread cache,
 * frames may reference strip ids that have no entry (or no `.strips` entry)
 * in the cached `screenMaps` dictionary. Rendering such a frame without a
 * layout produces an empty canvas.
 *
 * These functions are pure and side-effect free so they can be shared
 * between the background worker and other contexts without any DOM, worker,
 * or module dependencies. See issue #250 for the empty-canvas symptom this
 * module was introduced to fix.
 *
 * `screenMaps` shape (as produced by the C++/JS bridge and consumed by
 * graphics_manager.ts / graphics_manager_threejs.ts):
 *   {
 *     [stripId: string]: {
 *       strips: {
 *         [stripId: string]: { map: { x: number[], y: number[] }, diameter?: number, ... }
 *       },
 *       ...
 *     }
 *   }
 */

/**
 * Determines whether a screenmap layout is already cached for a given strip.
 *
 * Mirrors the lookup used by the renderers (graphics_manager.ts and
 * graphics_manager_threejs.ts): a strip has a usable layout only when
 * `screenMaps[stripId]` exists and its `.strips` sub-object has an own
 * property keyed by the same strip id.
 *
 * @param {{ [key: string]: { strips: { [key: string]: any } } }} screenMaps - Cached screenmap dictionary, keyed by strip id.
 * @param {string|number} stripId - Strip id to look up.
 * @returns {boolean} True when a layout for this strip is already cached.
 */
export function stripHasLayout(screenMaps, stripId) {
  const key = String(stripId);
  if (!screenMaps || typeof screenMaps !== 'object') {
    return false;
  }
  const screenMap = screenMaps[key];
  if (
    !screenMap
    || typeof screenMap !== 'object'
    || !screenMap.strips
    || typeof screenMap.strips !== 'object'
  ) {
    return false;
  }
  return Object.prototype.hasOwnProperty.call(screenMap.strips, key);
}

/**
 * Scans frame data and returns the strip ids that have no cached screenmap
 * layout yet.
 *
 * Frame entries use the field name `strip_id` (see extractFrameData() in
 * fastled_background_worker.ts and graphics_manager.ts). Entries missing a
 * `strip_id`, or that are falsy, are skipped. Duplicate strip ids are
 * reported only once, in first-seen order.
 *
 * @param {Array<{ strip_id?: (string|number) }>} frameData - Decoded per-strip frame entries for the current frame.
 * @param {{ [key: string]: { strips: { [key: string]: any } } }} screenMaps - Cached screenmap dictionary, keyed by strip id.
 * @returns {string[]} Strip ids (as strings) that have no cached layout, in first-seen order.
 */
export function findStripsMissingLayout(frameData, screenMaps) {
  if (!Array.isArray(frameData)) {
    return [];
  }

  const missing = [];
  const seen = new Set();

  for (const entry of frameData) {
    if (!entry || entry.strip_id === undefined || entry.strip_id === null) {
      continue;
    }
    const key = String(entry.strip_id);
    if (seen.has(key)) {
      continue;
    }
    if (!stripHasLayout(screenMaps, key)) {
      seen.add(key);
      missing.push(key);
    }
  }

  return missing;
}

/**
 * Convenience predicate: true when at least one strip referenced by the
 * frame has no cached screenmap layout yet.
 *
 * @param {Array<{ strip_id?: (string|number) }>} frameData - Decoded per-strip frame entries for the current frame.
 * @param {{ [key: string]: { strips: { [key: string]: any } } }} screenMaps - Cached screenmap dictionary, keyed by strip id.
 * @returns {boolean} True when the frame needs a screenmap refresh before it can be rendered.
 */
export function frameNeedsScreenMapRefresh(frameData, screenMaps) {
  return findStripsMissingLayout(frameData, screenMaps).length > 0;
}
