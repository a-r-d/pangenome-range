import { type FileHandle, open } from "node:fs/promises";
import type { RangeReadOptions, RangeSource } from "../reader/types.js";

export class FileRangeSource implements RangeSource {
  readonly #handle: FileHandle;
  #closed = false;

  private constructor(handle: FileHandle) {
    this.#handle = handle;
  }

  static async open(path: string | URL): Promise<FileRangeSource> {
    return new FileRangeSource(await open(path, "r"));
  }

  async size(signal?: AbortSignal): Promise<bigint> {
    this.#assertOpen();
    signal?.throwIfAborted();
    const metadata = await this.#handle.stat({ bigint: true });
    signal?.throwIfAborted();
    return metadata.size;
  }

  async read(
    offset: bigint,
    length: number,
    options?: RangeReadOptions,
  ): Promise<Uint8Array> {
    this.#assertOpen();
    if (offset < 0n) {
      throw new RangeError("archive offset must be non-negative");
    }
    if (!Number.isSafeInteger(length) || length < 0) {
      throw new RangeError("range length must be a non-negative safe integer");
    }
    options?.signal?.throwIfAborted();

    const bytes = new Uint8Array(length);
    let total = 0;
    while (total < length) {
      const result = await this.#handle.read(
        bytes,
        total,
        length - total,
        offset + BigInt(total),
      );
      if (result.bytesRead === 0) {
        throw new RangeError(
          `range ${offset}..${offset + BigInt(length)} extends past end of file`,
        );
      }
      total += result.bytesRead;
      options?.signal?.throwIfAborted();
    }
    return bytes;
  }

  async close(): Promise<void> {
    if (!this.#closed) {
      this.#closed = true;
      await this.#handle.close();
    }
  }

  #assertOpen(): void {
    if (this.#closed) {
      throw new Error("FileRangeSource is closed");
    }
  }
}
