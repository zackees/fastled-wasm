// @ts-nocheck
/// <reference path="types.d.ts" />

/**
 * Virtual filesystem bridge helpers for the FastLED WASM module.
 *
 * These helpers stream files from the browser into the Emscripten virtual
 * filesystem, partition files into immediate vs. streaming sets, and build
 * the file manifest JSON that the WASM module reads at startup.
 */

/**
 * Appends raw file data to WASM module file system
 * @param {Object} moduleInstance - The WASM module instance
 * @param {number} path_cstr - C string pointer to file path
 * @param {number} data_cbytes - C bytes pointer to file data
 * @param {number} len_int - Length of data in bytes
 */
export function jsAppendFileRaw(moduleInstance, path_cstr, data_cbytes, len_int) {
  // Stream this chunk
  moduleInstance.ccall(
    'jsAppendFile',
    'number', // return value
    ['number', 'number', 'number'], // argument types, not sure why numbers works.
    [path_cstr, data_cbytes, len_int],
  );
}

/**
 * Appends Uint8Array file data to WASM module file system
 * @param {Object} moduleInstance - The WASM module instance
 * @param {string} path - File path in the virtual file system
 * @param {Uint8Array} blob - File data as byte array
 */
export function jsAppendFileUint8(moduleInstance, path, blob) {
  const n = moduleInstance.lengthBytesUTF8(path) + 1;
  const path_cstr = moduleInstance._malloc(n);
  moduleInstance.stringToUTF8(path, path_cstr, n);
  const ptr = moduleInstance._malloc(blob.length);
  moduleInstance.HEAPU8.set(blob, ptr);
  jsAppendFileRaw(moduleInstance, path_cstr, ptr, blob.length);
  moduleInstance._free(ptr);
  moduleInstance._free(path_cstr);
}

/**
 * Partitions files into immediate and streaming categories based on extensions
 * @param {Array<Object>} filesJson - Array of file objects with path and data
 * @param {string[]} immediateExtensions - Extensions that should be loaded immediately
 * @returns {Array<Array<Object>>} [immediateFiles, streamingFiles]
 */
export function partition(filesJson, immediateExtensions) {
  const immediateFiles = [];
  const streamingFiles = [];
  filesJson.map((file) => {
    for (const ext of immediateExtensions) {
      const pathLower = file.path.toLowerCase();
      if (pathLower.endsWith(ext.toLowerCase())) {
        immediateFiles.push(file);
        return;
      }
    }
    streamingFiles.push(file);
  });
  return [immediateFiles, streamingFiles];
}

/**
 * Creates a file manifest JSON for the WASM module
 * @param {Array<Object>} filesJson - Array of file objects
 * @param {number} frame_rate - Target frame rate for animations
 * @returns {Object} Manifest object with files and frameRate
 */
export function getFileManifestJson(filesJson, frame_rate) {
  const trimmedFilesJson = filesJson.map((file) => ({
    path: file.path,
    size: file.size,
  }));
  const options = {
    files: trimmedFilesJson,
    frameRate: frame_rate,
  };
  return options;
}
