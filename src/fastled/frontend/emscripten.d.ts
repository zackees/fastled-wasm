/**
 * Emscripten Module global type definitions
 *
 * This file provides minimal type definitions for the Emscripten Module global
 * that is used throughout the WASM compiler codebase.
 */

declare const Module: {
  ccall: (name: string, returnType: string | null, argTypes: string[], args: any[]) => any;
  cwrap: (name: string, returnType: string | null, argTypes: string[]) => (...args: any[]) => any;
  _malloc: (size: number) => number;
  _free: (ptr: number) => void;
  getValue: (ptr: number, type: string) => number;
  setValue: (ptr: number, value: number, type: string) => void;
  stringToUTF8: (str: string, outPtr: number, maxBytesToWrite: number) => void;
  UTF8ToString: (ptr: number, maxBytesToRead?: number) => string;
  lengthBytesUTF8: (str: string) => number;
  HEAP8: Int8Array;
  HEAP16: Int16Array;
  HEAP32: Int32Array;
  HEAPU8: Uint8Array;
  HEAPU16: Uint16Array;
  HEAPU32: Uint32Array;
  HEAPF32: Float32Array;
  HEAPF64: Float64Array;
  canvas?: HTMLCanvasElement;
  onRuntimeInitialized?: () => void;
  [key: string]: any;
};
