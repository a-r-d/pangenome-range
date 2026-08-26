import {
  init as initWasm,
  decompress as wasmDecompress,
} from "@bokuweb/zstd-wasm";
import {
  type ChunkDecompressor,
  FzstdDecompressor,
} from "pangenome-range/reader";
import { TimedDecompressor } from "./metrics.js";
import type { DecoderName } from "./types.js";

class WasmZstdDecompressor implements ChunkDecompressor {
  decompress(
    compressed: Uint8Array,
    expectedLength: number,
    options?: { signal?: AbortSignal },
  ): Uint8Array {
    options?.signal?.throwIfAborted();
    const result = wasmDecompress(compressed);
    options?.signal?.throwIfAborted();
    if (result.byteLength !== expectedLength) {
      throw new RangeError(
        `WASM zstd decoded ${result.byteLength} bytes, expected ${expectedLength}`,
      );
    }
    return result;
  }
}

export interface InitializedDecoder {
  readonly name: DecoderName;
  readonly decompressor: TimedDecompressor;
  readonly initializationMs: number;
}

export async function initializeDecoder(
  name: DecoderName,
): Promise<InitializedDecoder> {
  const started = performance.now();
  let delegate: ChunkDecompressor;
  if (name === "wasm") {
    await initWasm();
    delegate = new WasmZstdDecompressor();
  } else {
    delegate = new FzstdDecompressor();
  }
  return {
    name,
    decompressor: new TimedDecompressor(delegate),
    initializationMs: performance.now() - started,
  };
}
