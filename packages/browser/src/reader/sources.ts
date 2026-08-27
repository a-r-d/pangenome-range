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
  cache?: RequestCache;
  useHead?: boolean;
  useIfRange?: boolean;
  /** Maximum incorrect 200 response accepted and sliced locally. Default: 0. */
  maxFullResponseBytes?: number;
}

export interface HttpRangeRequest {
  readonly method: "HEAD" | "GET";
  readonly offset?: bigint;
  readonly length?: number;
  readonly status: number;
  readonly responseBytes: number;
  readonly elapsedMs: number;
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

function parseContentLength(value: string | null): bigint | undefined {
  if (value === null || !/^\d+$/.test(value)) return undefined;
  return BigInt(value);
}

function elapsedSince(started: number): number {
  return performance.now() - started;
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

/** A strict HTTP source with bounded opt-in support for broken small origins. */
export class HttpRangeSource implements RangeSource {
  readonly #url: string;
  readonly #fetch: typeof globalThis.fetch;
  readonly #headers: Headers;
  readonly #cache: RequestCache | undefined;
  readonly #useHead: boolean;
  readonly #useIfRange: boolean;
  readonly #maxFullResponseBytes: number;
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
    if (this.#headers.has("range") || this.#headers.has("if-range")) {
      throw new TypeError("HttpRangeSource manages Range and If-Range headers");
    }
    this.#cache = options.cache;
    this.#useHead = options.useHead ?? true;
    this.#useIfRange = options.useIfRange ?? true;
    this.#maxFullResponseBytes = options.maxFullResponseBytes ?? 0;
    if (
      !Number.isSafeInteger(this.#maxFullResponseBytes) ||
      this.#maxFullResponseBytes < 0
    ) {
      throw new RangeError(
        "maxFullResponseBytes must be a non-negative safe integer",
      );
    }
  }

  get requests(): readonly HttpRangeRequest[] {
    return this.#requests.slice();
  }

  strongIdentity(): string | undefined {
    return this.#etag !== undefined && !this.#etag.startsWith("W/")
      ? this.#etag
      : undefined;
  }

  async size(signal?: AbortSignal): Promise<bigint> {
    signal?.throwIfAborted();
    if (this.#size === undefined) {
      const discovered =
        this.#useHead && (await this.#discoverWithHead(signal));
      if (!discovered) await this.#request(0n, 1, signal);
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
    return this.#request(offset, length, options?.signal, options?.cache);
  }

  async #discoverWithHead(signal?: AbortSignal): Promise<boolean> {
    signal?.throwIfAborted();
    const started = performance.now();
    const init: RequestInit = { method: "HEAD", headers: this.#headers };
    if (signal !== undefined) init.signal = signal;
    if (this.#cache !== undefined) init.cache = this.#cache;
    let response: Response;
    try {
      const fetch = this.#fetch;
      response = await fetch(this.#url, init);
    } catch {
      signal?.throwIfAborted();
      return false;
    }
    this.#requests.push({
      method: "HEAD",
      status: response.status,
      responseBytes: 0,
      elapsedMs: elapsedSince(started),
    });
    if (!response.ok) {
      response.body?.cancel().catch(() => undefined);
      return false;
    }
    const size = parseContentLength(response.headers.get("content-length"));
    const etag = response.headers.get("etag");
    if (
      size === undefined ||
      etag === null ||
      etag.length === 0 ||
      response.headers.get("accept-ranges")?.toLowerCase() !== "bytes"
    ) {
      response.body?.cancel().catch(() => undefined);
      return false;
    }
    this.#rememberIdentity(size, etag);
    return true;
  }

  async #request(
    offset: bigint,
    length: number,
    signal?: AbortSignal,
    cache?: RequestCache,
  ): Promise<Uint8Array> {
    assertRange(offset, length);
    signal?.throwIfAborted();
    const end = offset + BigInt(length) - 1n;
    const headers = new Headers(this.#headers);
    headers.set("Range", `bytes=${offset}-${end}`);
    if (this.#useIfRange && this.#etag !== undefined) {
      headers.set("If-Range", this.#etag);
    }
    const init: RequestInit = { headers };
    if (signal !== undefined) init.signal = signal;
    const effectiveCache = cache ?? this.#cache;
    if (effectiveCache !== undefined) init.cache = effectiveCache;
    const fetch = this.#fetch;
    const started = performance.now();
    const response = await fetch(this.#url, init);
    if (response.status === 200) {
      const contentLength = parseContentLength(
        response.headers.get("content-length"),
      );
      const etag = response.headers.get("etag");
      if (contentLength === undefined || etag === null || etag.length === 0) {
        response.body?.cancel().catch(() => undefined);
        throw new HttpRangeResponseError(
          "origin returned 200 without exposed Content-Length and ETag",
        );
      }
      try {
        this.#rememberIdentity(contentLength, etag);
      } catch (error) {
        response.body?.cancel().catch(() => undefined);
        throw error;
      }
      if (
        contentLength > BigInt(this.#maxFullResponseBytes) ||
        end >= contentLength
      ) {
        response.body?.cancel().catch(() => undefined);
        throw new HttpRangeResponseError(
          `origin ignored Range and returned a ${contentLength}-byte 200 response; maxFullResponseBytes is ${this.#maxFullResponseBytes}`,
        );
      }
      const full = new Uint8Array(await response.arrayBuffer());
      signal?.throwIfAborted();
      if (BigInt(full.byteLength) !== contentLength) {
        throw new HttpRangeResponseError(
          `whole-object body has ${full.byteLength} bytes, expected ${contentLength}`,
        );
      }
      this.#requests.push({
        method: "GET",
        offset,
        length,
        status: response.status,
        responseBytes: full.byteLength,
        elapsedMs: elapsedSince(started),
      });
      return full.slice(
        safeNumber(offset, "whole-object range offset"),
        safeNumber(end + 1n, "whole-object range end"),
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
    const parsedContentLength = parseContentLength(
      response.headers.get("content-length"),
    );
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
    try {
      this.#rememberIdentity(contentRange.size, etag);
    } catch (error) {
      response.body?.cancel().catch(() => undefined);
      throw error;
    }
    const bytes = new Uint8Array(await response.arrayBuffer());
    signal?.throwIfAborted();
    if (bytes.byteLength !== length) {
      throw new HttpRangeResponseError(
        `range body has ${bytes.byteLength} bytes, expected ${length}`,
      );
    }
    this.#requests.push({
      method: "GET",
      offset,
      length,
      status: response.status,
      responseBytes: bytes.byteLength,
      elapsedMs: elapsedSince(started),
    });
    return bytes;
  }

  #rememberIdentity(size: bigint, etag: string): void {
    if (this.#etag !== undefined && etag !== this.#etag) {
      throw new RemoteObjectChangedError(
        `remote object ETag changed from ${this.#etag} to ${etag}`,
      );
    }
    if (this.#size !== undefined && size !== this.#size) {
      throw new RemoteObjectChangedError(
        `remote object size changed from ${this.#size} to ${size}`,
      );
    }
    this.#etag = etag;
    this.#size = size;
  }
}

export interface TracedRangeRead {
  readonly sequence: number;
  readonly offset: bigint;
  readonly length: number;
  readonly succeeded: boolean;
  readonly elapsedMs: number;
  readonly bytesReturned: number;
  readonly layer: string;
}

export interface TracedByteRange {
  readonly offset: bigint;
  readonly length: number;
}

export interface TracedRangeSummary {
  readonly calls: number;
  readonly successfulCalls: number;
  readonly totalRequestedBytes: number;
  readonly returnedBytes: number;
  readonly uniqueBytes: number;
  readonly duplicateBytes: number;
  readonly coalescedRanges: readonly TracedByteRange[];
  readonly reads: readonly TracedRangeRead[];
}

function summarizeRanges(
  reads: readonly TracedRangeRead[],
): TracedRangeSummary {
  const intervals = reads
    .filter(({ length }) => length > 0)
    .map(({ offset, length }) => ({
      start: offset,
      end: offset + BigInt(length),
    }))
    .sort((left, right) =>
      left.start < right.start ? -1 : left.start > right.start ? 1 : 0,
    );
  const merged: Array<{ start: bigint; end: bigint }> = [];
  for (const interval of intervals) {
    const previous = merged.at(-1);
    if (previous !== undefined && interval.start <= previous.end) {
      if (interval.end > previous.end) previous.end = interval.end;
    } else {
      merged.push({ ...interval });
    }
  }
  const totalRequestedBytes = reads.reduce(
    (total, read) => total + read.length,
    0,
  );
  const uniqueBytes = merged.reduce(
    (total, range) =>
      total + safeNumber(range.end - range.start, "trace range"),
    0,
  );
  return {
    calls: reads.length,
    successfulCalls: reads.filter(({ succeeded }) => succeeded).length,
    totalRequestedBytes,
    returnedBytes: reads.reduce((total, read) => total + read.bytesReturned, 0),
    uniqueBytes,
    duplicateBytes: totalRequestedBytes - uniqueBytes,
    coalescedRanges: merged.map(({ start, end }) => ({
      offset: start,
      length: safeNumber(end - start, "trace range"),
    })),
    reads: [...reads].sort((left, right) => left.sequence - right.sequence),
  };
}

export class TracingRangeSource<T extends RangeSource = RangeSource>
  implements RangeSource
{
  readonly #source: T;
  readonly #layer: string;
  readonly #reads: TracedRangeRead[] = [];
  #nextSequence = 0;

  constructor(source: T, options: { layer?: string } = {}) {
    this.#source = source;
    this.#layer = options.layer ?? "source";
  }

  get source(): T {
    return this.#source;
  }

  get reads(): readonly TracedRangeRead[] {
    return [...this.#reads].sort(
      (left, right) => left.sequence - right.sequence,
    );
  }

  summary(): TracedRangeSummary {
    return summarizeRanges(this.#reads);
  }

  clear(): void {
    this.#reads.length = 0;
    this.#nextSequence = 0;
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
    const started = performance.now();
    try {
      const bytes = await this.#source.read(offset, length, options);
      this.#reads.push({
        sequence,
        offset,
        length,
        succeeded: true,
        elapsedMs: elapsedSince(started),
        bytesReturned: bytes.byteLength,
        layer: this.#layer,
      });
      return bytes;
    } catch (error) {
      this.#reads.push({
        sequence,
        offset,
        length,
        succeeded: false,
        elapsedMs: elapsedSince(started),
        bytesReturned: 0,
        layer: this.#layer,
      });
      throw error;
    }
  }

  close(): void | Promise<void> {
    return this.#source.close?.();
  }
}
