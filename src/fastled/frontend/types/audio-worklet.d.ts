/**
 * Ambient type declarations for AudioWorklet scope.
 *
 * These declarations cover the AudioWorklet global scope APIs that are
 * not part of the standard DOM lib but are available inside worklet contexts.
 */

declare class AudioWorkletProcessor {
  readonly port: MessagePort;
  constructor();
  process(
    inputs: Float32Array[][],
    outputs: Float32Array[][],
    parameters: Record<string, Float32Array>
  ): boolean;
}

declare function registerProcessor(
  name: string,
  processorCtor: typeof AudioWorkletProcessor
): void;
