import type { RangeReadOptions, RangeSource } from "./types.js";

const MAX_SAFE_BIGINT = BigInt(Number.MAX_SAFE_INTEGER);

export class HttpRangeResponseError extends Error {
  override readonly name = "HttpRangeResponseError";
}

export class RemoteObjectChangedError extends Error {
  override readonly name = "RemoteObjectChangedError";
}

export interface HttpRangeSourceOptions {
  fetch?: typeof globalThis.fetch;
  headers?: HeadersInit;
}

export interface HttpRangeRequest {
  readonly offset: bigint;
  readonly length: number;
  readonly status: number;
  readonly responseBytes: number;
}

function assertRange(offset: bigint, length: number): void {
  if (offset < 0n) {
    throw new RangeError("archive offset must be non-negative");
  }
  if (!Number.isSafeInteger(length) || length < 0) {
    throw new RangeError("range length must be a non-negative safe integer");
  }
  const end = offset + BigInt(length);
  if (end < offset) {
    throw new RangeError("archive range end overflow");
  }
}

function safeNumber(value: bigint, label: string): number {
  if (value < 0n || value > MAX_SAFE_BIGINT) {
    throw new RangeError(`${label} exceeds JavaScript's safe integer range`);
  }
  return Number(value);
}

export class MemoryRangeSource implements RangeSource {
  readonly #bytes: Uint8Array;

  constructor(bytes: Uint8Array, options: { copy?: boolean } = {}) {
    this.#bytes = options.copy === false ? bytes : bytes.slice();
  }

  async size(signal?: AbortSignal): Promise<bigint> {
    signal?.throwIfAborted();
    return BigInt(this.#bytes.byteLength);
  }

  async read(
    offset: bigint,
    length: number,
    options?: RangeReadOptions,
  ): Promise<Uint8Array> {
    assertRange(offset, length);
    options?.signal?.throwIfAborted();
    const start = safeNumber(offset, "memory range offset");
    const end = start + length;
    if (!Number.isSafeInteger(end) || end > this.#bytes.byteLength) {
      throw new RangeError(
        `range ${offset}..${offset + BigInt(length)} exceeds source size`,
      );
    }
    return this.#bytes.slice(start, end);
  }
}

export class BlobRangeSource implements RangeSource {
  readonly #blob: Blob;

  constructor(blob: Blob) {
    this.#blob = blob;
  }

  async size(signal?: AbortSignal): Promise<bigint> {
    signal?.throwIfAborted();
    return BigInt(this.#blob.size);
  }

  async read(
    offset: bigint,
    length: number,
    options?: RangeReadOptions,
  ): Promise<Uint8Array> {
    assertRange(offset, length);
    options?.signal?.throwIfAborted();
    const start = safeNumber(offset, "blob range offset");
    const end = start + length;
    if (!Number.isSafeInteger(end) || end > this.#blob.size) {
      throw new RangeError(
        `range ${offset}..${offset + BigInt(length)} exceeds source size`,
      );
    }
    const bytes = new Uint8Array(
      await this.#blob.slice(start, end).arrayBuffer(),
    );
    options?.signal?.throwIfAborted();
    if (bytes.byteLength !== length) {
      throw new HttpRangeResponseError("Blob returned a short range");
    }
    return bytes;
  }
}

interface ParsedContentRange {
  start: bigint;
  end: bigint;
  size: bigint;
}

function parseContentRange(value: string | null): ParsedContentRange {
  const match = /^bytes (\d+)-(\d+)\/(\d+)$/.exec(value ?? "");
  if (match === null) {
    throw new HttpRangeResponseError(
      `expected an exposed Content-Range header, received ${JSON.stringify(value)}`,
    );
  }
  const start = BigInt(match[1] as string);
  const end = BigInt(match[2] as string);
  const size = BigInt(match[3] as string);
  if (start > end || end >= size) {
    throw new HttpRangeResponseError(
      "Content-Range contains an invalid interval",
    );
  }
  return { start, end, size };
}

/** A strict HTTP source that never accepts whole-object fallback responses. */
export class HttpRangeSource implements RangeSource {
  readonly #url: string;
  readonly #fetch: typeof globalThis.fetch;
  readonly #headers: Headers;
  #size?: bigint;
  #etag?: string;
  readonly #requests: HttpRangeRequest[] = [];

  constructor(url: string | URL, options: HttpRangeSourceOptions = {}) {
    this.#url = String(url);
    this.#fetch = options.fetch ?? globalThis.fetch;
    if (typeof this.#fetch !== "function") {
      throw new TypeError("HttpRangeSource requires a fetch implementation");
    }
    this.#headers = new Headers(options.headers);
    if (this.#headers.has("range")) {
      throw new TypeError("HttpRangeSource manages the Range header");
    }
  }

  get requests(): readonly HttpRangeRequest[] {
    return this.#requests.slice();
  }

  async size(signal?: AbortSignal): Promise<bigint> {
    signal?.throwIfAborted();
    if (this.#size === undefined) {
      await this.#request(0n, 1, signal);
    }
    return this.#size as bigint;
  }

  async read(
    offset: bigint,
    length: number,
    options?: RangeReadOptions,
  ): Promise<Uint8Array> {
    assertRange(offset, length);
    options?.signal?.throwIfAborted();
    if (length === 0) return new Uint8Array();
    if (this.#size !== undefined && offset + BigInt(length) > this.#size) {
      throw new RangeError(
        `range ${offset}..${offset + BigInt(length)} exceeds source size`,
      );
    }
    return this.#request(offset, length, options?.signal);
  }

  async #request(
    offset: bigint,
    length: number,
    signal?: AbortSignal,
  ): Promise<Uint8Array> {
    assertRange(offset, length);
    signal?.throwIfAborted();
    const end = offset + BigInt(length) - 1n;
    const headers = new Headers(this.#headers);
    headers.set("Range", `bytes=${offset}-${end}`);
    const init: RequestInit = { headers };
    if (signal !== undefined) init.signal = signal;
    const fetch = this.#fetch;
    const response = await fetch(this.#url, init);
    if (response.status === 200) {
      response.body?.cancel().catch(() => undefined);
      throw new HttpRangeResponseError(
        "origin ignored Range and returned 200; refusing to download the whole object",
      );
    }
    if (response.status !== 206) {
      response.body?.cancel().catch(() => undefined);
      throw new HttpRangeResponseError(
        `range request returned HTTP ${response.status}, expected 206`,
      );
    }
    if (response.headers.get("accept-ranges")?.toLowerCase() !== "bytes") {
      response.body?.cancel().catch(() => undefined);
      throw new HttpRangeResponseError(
        "range response is missing the exposed Accept-Ranges: bytes header",
      );
    }
    const contentRange = parseContentRange(
      response.headers.get("content-range"),
    );
    if (
      contentRange.start !== offset ||
      contentRange.end !== end ||
      contentRange.size <= end
    ) {
      response.body?.cancel().catch(() => undefined);
      throw new HttpRangeResponseError(
        `Content-Range does not match requested bytes ${offset}-${end}`,
      );
    }
    const contentLength = response.headers.get("content-length");
    let parsedContentLength: bigint | undefined;
    try {
      parsedContentLength =
        contentLength === null ? undefined : BigInt(contentLength);
    } catch {
      // Report malformed transport metadata through the public source error.
    }
    if (parsedContentLength !== BigInt(length)) {
      response.body?.cancel().catch(() => undefined);
      throw new HttpRangeResponseError(
        `Content-Length does not match requested length ${length}`,
      );
    }
    const etag = response.headers.get("etag");
    if (etag === null || etag.length === 0) {
      response.body?.cancel().catch(() => undefined);
      throw new HttpRangeResponseError(
        "range response is missing an exposed stable ETag",
      );
    }
    if (this.#etag !== undefined && etag !== this.#etag) {
      response.body?.cancel().catch(() => undefined);
      throw new RemoteObjectChangedError(
        `remote object ETag changed from ${this.#etag} to ${etag}`,
      );
    }
    this.#etag = etag;
    if (this.#size !== undefined && contentRange.size !== this.#size) {
      response.body?.cancel().catch(() => undefined);
      throw new RemoteObjectChangedError(
        `remote object size changed from ${this.#size} to ${contentRange.size}`,
      );
    }
    this.#size = contentRange.size;
    const bytes = new Uint8Array(await response.arrayBuffer());
    signal?.throwIfAborted();
    if (bytes.byteLength !== length) {
      throw new HttpRangeResponseError(
        `range body has ${bytes.byteLength} bytes, expected ${length}`,
      );
    }
    this.#requests.push({
      offset,
      length,
      status: response.status,
      responseBytes: bytes.byteLength,
    });
    return bytes;
  }
}

export interface TracedRangeRead {
  readonly sequence: number;
  readonly offset: bigint;
  readonly length: number;
  readonly succeeded: boolean;
}

export class TracingRangeSource implements RangeSource {
  readonly #source: RangeSource;
  readonly #reads: TracedRangeRead[] = [];
  #nextSequence = 0;

  constructor(source: RangeSource) {
    this.#source = source;
  }

  get reads(): readonly TracedRangeRead[] {
    return this.#reads.slice();
  }

  async size(signal?: AbortSignal): Promise<bigint> {
    return this.#source.size(signal);
  }

  async read(
    offset: bigint,
    length: number,
    options?: RangeReadOptions,
  ): Promise<Uint8Array> {
    const sequence = this.#nextSequence;
    this.#nextSequence += 1;
    try {
      const bytes = await this.#source.read(offset, length, options);
      this.#reads.push({ sequence, offset, length, succeeded: true });
      return bytes;
    } catch (error) {
      this.#reads.push({ sequence, offset, length, succeeded: false });
      throw error;
    }
  }

  close(): void | Promise<void> {
    return this.#source.close?.();
  }
}
