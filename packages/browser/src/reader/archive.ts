import { decompress as fzstdDecompress } from "fzstd";
import { decodeRegionalPayload } from "./regional.js";
import { BlobRangeSource, HttpRangeSource } from "./sources.js";
import type {
  ChunkDecompressor,
  OpenPangenomeOptions,
  PangenomeArchive,
  RangeReadOptions,
  RangeSource,
  ReferenceDescriptor,
  RegionQuery,
  RegionResult,
  RegionTile,
} from "./types.js";

const ARCHIVE_MAGIC = "PNGRNG04";
const ARCHIVE_VERSION = 4;
const HEADER_BYTES = 64;
const BOOTSTRAP_BYTES = 16 * 1024;
const DIRECTORY_PAGE_BYTES = 4 * 1024;
const DIRECTORY_ENTRY_BYTES = 40;
const DIRECTORY_ENTRY_CAPACITY = 102;
const DEFAULT_DIRECTORY_CACHE_BYTES = 1024 * 1024;
const DEFAULT_PAYLOAD_CACHE_BYTES = 32 * 1024 * 1024;
const DEFAULT_MAX_ROOT_BYTES = 16 * 1024 * 1024;
const DEFAULT_MAX_CHUNK_BYTES = 64 * 1024 * 1024;
const MAX_SAFE_BIGINT = BigInt(Number.MAX_SAFE_INTEGER);
const textDecoder = new TextDecoder("utf-8", { fatal: true });

export class CorruptArchiveError extends Error {
  override readonly name = "CorruptArchiveError";
}

export class UnsupportedArchiveVersionError extends Error {
  override readonly name = "UnsupportedArchiveVersionError";
}

export class UnsupportedChunkCodecError extends Error {
  override readonly name = "UnsupportedChunkCodecError";
}

type ChunkCodec = 0 | 1 | 3 | 6;

interface Header {
  rootLength: bigint;
  entryCount: bigint;
  dataOffset: bigint;
}

interface Manifest {
  sample: string;
  contig: string;
  start: bigint;
  end: bigint;
  gridStart: bigint;
  windowSize: bigint;
  bucketSpan: bigint;
  firstPageOffset: bigint;
  pageCount: bigint;
  entryCount: bigint;
  codec: ChunkCodec;
}

interface DirectoryEntry {
  manifest: Manifest;
  start: bigint;
  end: bigint;
  offset: bigint;
  compressedLength: bigint;
  uncompressedLength: bigint;
}

class BinaryReader {
  readonly #bytes: Uint8Array;
  readonly #view: DataView;
  #position = 0;

  constructor(bytes: Uint8Array) {
    this.#bytes = bytes;
    this.#view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  }

  get remaining(): number {
    return this.#bytes.byteLength - this.#position;
  }

  take(length: number): Uint8Array {
    if (!Number.isSafeInteger(length) || length < 0) {
      throw corrupt("invalid archive section length");
    }
    const end = this.#position + length;
    if (!Number.isSafeInteger(end) || end > this.#bytes.byteLength) {
      throw corrupt("unexpected end of archive metadata");
    }
    const result = this.#bytes.subarray(this.#position, end);
    this.#position = end;
    return result;
  }

  u8(): number {
    return this.take(1)[0] as number;
  }

  u32(): number {
    const position = this.#position;
    this.take(4);
    return this.#view.getUint32(position, true);
  }

  u64(): bigint {
    const position = this.#position;
    this.take(8);
    return this.#view.getBigUint64(position, true);
  }

  string(): string {
    const length = safeNumber(this.u64(), "archive string length");
    try {
      return textDecoder.decode(this.take(length));
    } catch (error) {
      throw corrupt(`invalid UTF-8 archive string: ${String(error)}`);
    }
  }

  finish(): void {
    if (this.remaining !== 0) {
      throw corrupt(`${this.remaining} trailing archive metadata bytes`);
    }
  }
}

class ByteCache {
  readonly #limit: number;
  readonly #values = new Map<string, Uint8Array>();
  #bytes = 0;

  constructor(limit: number) {
    this.#limit = limit;
  }

  get(key: string): Uint8Array | undefined {
    const value = this.#values.get(key);
    if (value !== undefined) {
      this.#values.delete(key);
      this.#values.set(key, value);
    }
    return value;
  }

  set(key: string, value: Uint8Array): void {
    if (this.#limit === 0 || value.byteLength > this.#limit) return;
    const existing = this.#values.get(key);
    if (existing !== undefined) {
      this.#bytes -= existing.byteLength;
      this.#values.delete(key);
    }
    while (this.#bytes + value.byteLength > this.#limit) {
      const oldest = this.#values.entries().next().value as
        | [string, Uint8Array]
        | undefined;
      if (oldest === undefined) break;
      this.#values.delete(oldest[0]);
      this.#bytes -= oldest[1].byteLength;
    }
    this.#values.set(key, value);
    this.#bytes += value.byteLength;
  }
}

function corrupt(message: string): CorruptArchiveError {
  return new CorruptArchiveError(message);
}

function safeNumber(value: bigint, label: string): number {
  if (value < 0n || value > MAX_SAFE_BIGINT) {
    throw corrupt(`${label} exceeds JavaScript's safe integer range`);
  }
  return Number(value);
}

function assertZero(bytes: Uint8Array, label: string): void {
  if (bytes.some((value) => value !== 0)) {
    throw corrupt(`${label} must be zero`);
  }
}

function checkedAdd(left: bigint, right: bigint, label: string): bigint {
  const result = left + right;
  if (result > 0xffff_ffff_ffff_ffffn) {
    throw corrupt(`${label} overflows u64`);
  }
  return result;
}

function checkedMultiply(left: bigint, right: bigint, label: string): bigint {
  const result = left * right;
  if (result > 0xffff_ffff_ffff_ffffn) {
    throw corrupt(`${label} overflows u64`);
  }
  return result;
}

function decodeHeader(bytes: Uint8Array): Header {
  if (bytes.byteLength !== HEADER_BYTES) {
    throw corrupt("archive header has the wrong size");
  }
  const reader = new BinaryReader(bytes);
  const magic = textDecoder.decode(reader.take(8));
  const version = reader.u32();
  const headerLength = reader.u32();
  const rootOffset = reader.u64();
  const rootLength = reader.u64();
  const entryCount = reader.u64();
  const dataOffset = reader.u64();
  assertZero(reader.take(16), "archive header reserved bytes");
  if (
    magic !== ARCHIVE_MAGIC ||
    version !== ARCHIVE_VERSION ||
    headerLength !== HEADER_BYTES ||
    rootOffset !== BigInt(HEADER_BYTES)
  ) {
    throw new UnsupportedArchiveVersionError(
      `unsupported archive magic ${JSON.stringify(magic)}, version ${version}, header length ${headerLength}, or root offset ${rootOffset}`,
    );
  }
  return { rootLength, entryCount, dataOffset };
}

function decodeCodec(value: number): ChunkCodec {
  if (value === 0 || value === 1 || value === 3 || value === 6) return value;
  throw new UnsupportedChunkCodecError(
    `unsupported archive chunk codec ${value}`,
  );
}

function decodeRoot(bytes: Uint8Array, header: Header): Manifest[] {
  const reader = new BinaryReader(bytes);
  const count = safeNumber(reader.u64(), "reference manifest count");
  if (count > Math.floor(reader.remaining / 80)) {
    throw corrupt("reference manifest count exceeds root bytes");
  }
  const manifests: Manifest[] = [];
  const rootEnd = checkedAdd(
    BigInt(HEADER_BYTES),
    header.rootLength,
    "root end",
  );
  let previousPageEnd = rootEnd;
  let totalEntries = 0n;
  for (let index = 0; index < count; index += 1) {
    const manifest: Manifest = {
      sample: reader.string(),
      contig: reader.string(),
      start: reader.u64(),
      end: reader.u64(),
      gridStart: reader.u64(),
      windowSize: reader.u64(),
      bucketSpan: reader.u64(),
      firstPageOffset: reader.u64(),
      pageCount: reader.u64(),
      entryCount: reader.u64(),
      codec: decodeCodec(reader.u8()),
    };
    assertZero(reader.take(7), "reference manifest reserved bytes");
    if (manifest.sample.length === 0 || manifest.contig.length === 0) {
      throw corrupt("reference manifest identity is empty");
    }
    if (
      manifest.start >= manifest.end ||
      manifest.gridStart > manifest.start ||
      manifest.windowSize === 0n ||
      manifest.bucketSpan === 0n ||
      manifest.pageCount === 0n
    ) {
      throw corrupt("invalid arithmetic reference manifest");
    }
    const span = manifest.end - manifest.gridStart;
    const expectedPages =
      (span + manifest.bucketSpan - 1n) / manifest.bucketSpan;
    const pageBytes = checkedMultiply(
      manifest.pageCount,
      BigInt(DIRECTORY_PAGE_BYTES),
      "manifest page range",
    );
    const pageEnd = checkedAdd(
      manifest.firstPageOffset,
      pageBytes,
      "manifest page end",
    );
    if (
      manifest.pageCount !== expectedPages ||
      manifest.firstPageOffset !== previousPageEnd ||
      pageEnd > header.dataOffset
    ) {
      throw corrupt("reference manifest directory range is inconsistent");
    }
    totalEntries = checkedAdd(
      totalEntries,
      manifest.entryCount,
      "directory entry count",
    );
    previousPageEnd = pageEnd;
    manifests.push(manifest);
  }
  reader.finish();
  if (
    totalEntries !== header.entryCount ||
    previousPageEnd !== header.dataOffset
  ) {
    throw corrupt(
      "root manifests differ from header entry count or data offset",
    );
  }
  return manifests;
}

function decodeDirectoryPage(
  bytes: Uint8Array,
  manifest: Manifest,
  bucketIndex: bigint,
  dataOffset: bigint,
  sourceSize: bigint,
  maxChunkBytes: number,
): DirectoryEntry[] {
  if (bytes.byteLength !== DIRECTORY_PAGE_BYTES) {
    throw corrupt("directory page has the wrong size");
  }
  const reader = new BinaryReader(bytes);
  const count = reader.u32();
  const entryBytes = reader.u32();
  const bucketStart = reader.u64();
  const expectedBucketStart = checkedAdd(
    manifest.gridStart,
    checkedMultiply(
      bucketIndex,
      manifest.bucketSpan,
      "directory bucket offset",
    ),
    "directory bucket start",
  );
  if (
    count > DIRECTORY_ENTRY_CAPACITY ||
    entryBytes !== DIRECTORY_ENTRY_BYTES ||
    bucketStart !== expectedBucketStart
  ) {
    throw corrupt("invalid fixed directory page header");
  }
  const bucketEnd = checkedAdd(
    bucketStart,
    manifest.bucketSpan,
    "directory bucket end",
  );
  const entries: DirectoryEntry[] = [];
  for (let index = 0; index < count; index += 1) {
    const start = reader.u64();
    const end = reader.u64();
    const offset = reader.u64();
    const compressedLength = reader.u64();
    const uncompressedLength = reader.u64();
    const payloadEnd = checkedAdd(offset, compressedLength, "payload end");
    if (
      start >= end ||
      start < bucketStart ||
      end > (bucketEnd < manifest.end ? bucketEnd : manifest.end) ||
      compressedLength === 0n ||
      uncompressedLength === 0n ||
      offset < dataOffset ||
      payloadEnd > sourceSize ||
      compressedLength > BigInt(maxChunkBytes) ||
      uncompressedLength > BigInt(maxChunkBytes)
    ) {
      throw corrupt("invalid fixed directory entry");
    }
    entries.push({
      manifest,
      start,
      end,
      offset,
      compressedLength,
      uncompressedLength,
    });
  }
  assertZero(reader.take(reader.remaining), "directory page padding");
  return entries;
}

function assertNonNegativeSafeInteger(value: number, label: string): void {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new RangeError(`${label} must be a non-negative safe integer`);
  }
}

function signalOptions(signal?: AbortSignal): RangeReadOptions | undefined {
  return signal === undefined ? undefined : { signal };
}

function validateQuery(query: RegionQuery): void {
  if (query.sample.length === 0 || query.contig.length === 0) {
    throw new TypeError("query sample and contig must not be empty");
  }
  assertNonNegativeSafeInteger(query.start, "query.start");
  assertNonNegativeSafeInteger(query.end, "query.end");
  if (query.end <= query.start) {
    throw new RangeError("query.end must be greater than query.start");
  }
  if (query.context !== undefined) {
    assertNonNegativeSafeInteger(query.context, "query.context");
  }
  query.signal?.throwIfAborted();
}

function isRangeSource(value: unknown): value is RangeSource {
  return (
    typeof value === "object" &&
    value !== null &&
    "size" in value &&
    typeof value.size === "function" &&
    "read" in value &&
    typeof value.read === "function"
  );
}

export class FzstdDecompressor implements ChunkDecompressor {
  decompress(
    compressed: Uint8Array,
    expectedLength: number,
    options?: { signal?: AbortSignal },
  ): Uint8Array {
    assertNonNegativeSafeInteger(
      expectedLength,
      "expected decompressed length",
    );
    options?.signal?.throwIfAborted();
    let result: Uint8Array;
    try {
      result = fzstdDecompress(compressed, new Uint8Array(expectedLength));
    } catch (error) {
      throw corrupt(`zstd decompression failed: ${String(error)}`);
    }
    options?.signal?.throwIfAborted();
    if (result.byteLength !== expectedLength) {
      throw corrupt(
        `zstd payload decoded to ${result.byteLength} bytes, expected ${expectedLength}`,
      );
    }
    return result;
  }
}

class ArchiveReader implements PangenomeArchive {
  readonly formatVersion = 4 as const;
  readonly #source: RangeSource;
  readonly #sourceSize: bigint;
  readonly #bootstrap: Uint8Array;
  readonly #header: Header;
  readonly #manifests: Manifest[];
  readonly #directoryCache: ByteCache;
  readonly #payloadCache: ByteCache;
  readonly #decompressor: ChunkDecompressor;
  readonly #maxChunkBytes: number;
  #closed = false;

  constructor(
    source: RangeSource,
    sourceSize: bigint,
    bootstrap: Uint8Array,
    header: Header,
    manifests: Manifest[],
    options: OpenPangenomeOptions,
  ) {
    this.#source = source;
    this.#sourceSize = sourceSize;
    this.#bootstrap = bootstrap;
    this.#header = header;
    this.#manifests = manifests;
    this.#directoryCache = new ByteCache(
      options.directoryCacheBytes ?? DEFAULT_DIRECTORY_CACHE_BYTES,
    );
    this.#payloadCache = new ByteCache(
      options.payloadCacheBytes ?? DEFAULT_PAYLOAD_CACHE_BYTES,
    );
    this.#decompressor = options.decompressor ?? new FzstdDecompressor();
    this.#maxChunkBytes = options.maxChunkBytes ?? DEFAULT_MAX_CHUNK_BYTES;
  }

  references(): readonly ReferenceDescriptor[] {
    this.#assertOpen();
    return this.#manifests.map((manifest) => ({
      sample: manifest.sample,
      contig: manifest.contig,
      start: safeNumber(manifest.start, "reference start"),
      end: safeNumber(manifest.end, "reference end"),
      orientation: "forward",
    }));
  }

  async query(query: RegionQuery): Promise<RegionResult> {
    const tiles: RegionTile[] = [];
    for await (const tile of this.queryTiles(query)) tiles.push(tile);
    tiles.sort(
      (left, right) => left.start - right.start || left.end - right.end,
    );
    return {
      query: { ...query },
      semantics: "anonymous-distinct-weighted-tile-paths",
      tiles,
    };
  }

  async *queryTiles(query: RegionQuery): AsyncIterable<RegionTile> {
    this.#assertOpen();
    validateQuery(query);
    const entries = await this.#lookup(query);
    const uniqueEntries = new Map<string, DirectoryEntry>();
    for (const entry of entries) {
      uniqueEntries.set(
        `${entry.offset}:${entry.compressedLength}:${entry.uncompressedLength}`,
        entry,
      );
    }
    const pending = Array.from(uniqueEntries.values(), (entry, index) => ({
      index,
      promise: this.#loadTile(entry, query.signal).then((tile) => ({
        index,
        tile,
      })),
    }));
    while (pending.length > 0) {
      const completed = await Promise.race(pending.map((item) => item.promise));
      const index = pending.findIndex((item) => item.index === completed.index);
      pending.splice(index, 1);
      yield completed.tile;
    }
  }

  async close(): Promise<void> {
    if (!this.#closed) {
      this.#closed = true;
      await this.#source.close?.();
    }
  }

  async #lookup(query: RegionQuery): Promise<DirectoryEntry[]> {
    const queryStart = BigInt(query.start);
    const queryEnd = BigInt(query.end);
    const matching = this.#manifests.filter(
      (manifest) =>
        manifest.sample === query.sample &&
        manifest.contig === query.contig &&
        manifest.start < queryEnd &&
        manifest.end > queryStart,
    );
    if (matching.length === 0) {
      throw new RangeError(
        `archive has no reference interval for ${query.sample}#${query.contig}:${query.start}-${query.end}`,
      );
    }
    const groups = await Promise.all(
      matching.map(async (manifest) => {
        const selectionStart =
          queryStart > manifest.start ? queryStart : manifest.start;
        const selectionEnd = queryEnd < manifest.end ? queryEnd : manifest.end;
        const firstBucket =
          (selectionStart - manifest.gridStart) / manifest.bucketSpan;
        const lastBucket =
          (selectionEnd - 1n - manifest.gridStart) / manifest.bucketSpan;
        const pages = await this.#loadDirectoryPages(
          manifest,
          firstBucket,
          lastBucket,
          query.signal,
        );
        return pages
          .flatMap(({ bytes, bucketIndex }) =>
            decodeDirectoryPage(
              bytes,
              manifest,
              bucketIndex,
              this.#header.dataOffset,
              this.#sourceSize,
              this.#maxChunkBytes,
            ),
          )
          .filter((entry) => entry.start < queryEnd && entry.end > queryStart);
      }),
    );
    const entries = groups.flat();
    if (entries.length === 0) {
      throw corrupt(
        "matching reference interval has no regional payload entries",
      );
    }
    return entries;
  }

  async #loadDirectoryPages(
    manifest: Manifest,
    firstBucket: bigint,
    lastBucket: bigint,
    signal?: AbortSignal,
  ): Promise<Array<{ bytes: Uint8Array; bucketIndex: bigint }>> {
    if (
      firstBucket < 0n ||
      lastBucket < firstBucket ||
      lastBucket >= manifest.pageCount
    ) {
      throw corrupt("directory bucket selection is outside its manifest");
    }
    const pageCount = safeNumber(
      lastBucket - firstBucket + 1n,
      "directory page count",
    );
    const pages: Array<{ bytes: Uint8Array; bucketIndex: bigint }> = [];
    let allCached = true;
    for (let index = 0; index < pageCount; index += 1) {
      const bucketIndex = firstBucket + BigInt(index);
      const offset = checkedAdd(
        manifest.firstPageOffset,
        checkedMultiply(
          bucketIndex,
          BigInt(DIRECTORY_PAGE_BYTES),
          "page offset",
        ),
        "page offset",
      );
      const cached = this.#directoryCache.get(String(offset));
      if (cached === undefined) allCached = false;
      pages.push({ bytes: cached ?? new Uint8Array(), bucketIndex });
    }
    if (allCached) return pages;

    const firstOffset = checkedAdd(
      manifest.firstPageOffset,
      checkedMultiply(
        firstBucket,
        BigInt(DIRECTORY_PAGE_BYTES),
        "page span start",
      ),
      "page span start",
    );
    const totalBytes = pageCount * DIRECTORY_PAGE_BYTES;
    const fetched = await this.#readStored(firstOffset, totalBytes, signal);
    for (let index = 0; index < pageCount; index += 1) {
      const start = index * DIRECTORY_PAGE_BYTES;
      const page = fetched.slice(start, start + DIRECTORY_PAGE_BYTES);
      const offset = firstOffset + BigInt(start);
      this.#directoryCache.set(String(offset), page);
      pages[index] = {
        bytes: page,
        bucketIndex: firstBucket + BigInt(index),
      };
    }
    return pages;
  }

  async #loadTile(
    entry: DirectoryEntry,
    signal?: AbortSignal,
  ): Promise<RegionTile> {
    signal?.throwIfAborted();
    const key = `${entry.offset}:${entry.compressedLength}`;
    let compressed = this.#payloadCache.get(key);
    if (compressed === undefined) {
      compressed = await this.#readStored(
        entry.offset,
        safeNumber(entry.compressedLength, "compressed payload length"),
        signal,
      );
      this.#payloadCache.set(key, compressed);
    }
    const expectedLength = safeNumber(
      entry.uncompressedLength,
      "uncompressed payload length",
    );
    const raw =
      entry.manifest.codec === 0
        ? compressed.slice()
        : await this.#decompressor.decompress(
            compressed,
            expectedLength,
            signalOptions(signal),
          );
    signal?.throwIfAborted();
    if (raw.byteLength !== expectedLength) {
      throw corrupt(
        `regional payload has ${raw.byteLength} bytes after decompression, expected ${expectedLength}`,
      );
    }
    const tile = decodeRegionalPayload(raw, {
      archiveOffset: entry.offset,
      encodedLength: safeNumber(
        entry.compressedLength,
        "encoded payload length",
      ),
    });
    if (
      BigInt(tile.start) !== entry.start ||
      BigInt(tile.end) !== entry.end ||
      tile.reference.sample !== entry.manifest.sample ||
      tile.reference.contig !== entry.manifest.contig
    ) {
      throw corrupt(
        "regional payload provenance differs from its directory entry",
      );
    }
    return tile;
  }

  async #readStored(
    offset: bigint,
    length: number,
    signal?: AbortSignal,
  ): Promise<Uint8Array> {
    assertNonNegativeSafeInteger(length, "stored range length");
    const end = checkedAdd(offset, BigInt(length), "stored range end");
    if (end > this.#sourceSize) {
      throw corrupt("stored range is outside the archive");
    }
    const bootstrapEnd = BigInt(this.#bootstrap.byteLength);
    if (end <= bootstrapEnd) {
      return this.#bootstrap.slice(
        safeNumber(offset, "bootstrap range offset"),
        safeNumber(end, "bootstrap range end"),
      );
    }
    if (offset >= bootstrapEnd) {
      return this.#source.read(offset, length, signalOptions(signal));
    }
    const prefix = this.#bootstrap.slice(
      safeNumber(offset, "bootstrap range offset"),
    );
    const suffix = await this.#source.read(
      bootstrapEnd,
      safeNumber(end - bootstrapEnd, "stored range suffix"),
      signalOptions(signal),
    );
    const result = new Uint8Array(length);
    result.set(prefix);
    result.set(suffix, prefix.byteLength);
    return result;
  }

  #assertOpen(): void {
    if (this.#closed) throw new Error("PangenomeArchive is closed");
  }
}

function createSource(options: OpenPangenomeOptions): RangeSource {
  if (typeof options.source === "string") {
    return new HttpRangeSource(
      options.source,
      options.fetch === undefined ? {} : { fetch: options.fetch },
    );
  }
  if (isRangeSource(options.source)) return options.source;
  if (options.source instanceof Blob)
    return new BlobRangeSource(options.source);
  throw new TypeError("source must be a URL string, Blob, or RangeSource");
}

export async function openPangenomeArchive(
  options: OpenPangenomeOptions,
): Promise<PangenomeArchive> {
  options.signal?.throwIfAborted();
  const directoryCacheBytes =
    options.directoryCacheBytes ?? DEFAULT_DIRECTORY_CACHE_BYTES;
  const payloadCacheBytes =
    options.payloadCacheBytes ?? DEFAULT_PAYLOAD_CACHE_BYTES;
  const maxRootBytes = options.maxRootBytes ?? DEFAULT_MAX_ROOT_BYTES;
  const maxChunkBytes = options.maxChunkBytes ?? DEFAULT_MAX_CHUNK_BYTES;
  for (const [value, label] of [
    [directoryCacheBytes, "directoryCacheBytes"],
    [payloadCacheBytes, "payloadCacheBytes"],
    [maxRootBytes, "maxRootBytes"],
    [maxChunkBytes, "maxChunkBytes"],
  ] as const) {
    assertNonNegativeSafeInteger(value, label);
  }
  if (maxRootBytes < HEADER_BYTES || maxChunkBytes === 0) {
    throw new RangeError("maxRootBytes is too small or maxChunkBytes is zero");
  }
  const source = createSource(options);
  try {
    const sourceSize = await source.size(options.signal);
    if (sourceSize < BigInt(HEADER_BYTES)) {
      throw corrupt("archive is shorter than its header");
    }
    const firstLength = safeNumber(
      sourceSize < BigInt(BOOTSTRAP_BYTES)
        ? sourceSize
        : BigInt(BOOTSTRAP_BYTES),
      "bootstrap length",
    );
    let bootstrap = await source.read(0n, firstLength, {
      ...(options.signal === undefined ? {} : { signal: options.signal }),
    });
    const header = decodeHeader(bootstrap.subarray(0, HEADER_BYTES));
    if (header.rootLength > BigInt(maxRootBytes)) {
      throw corrupt(
        `root index is ${header.rootLength} bytes, above maxRootBytes ${maxRootBytes}`,
      );
    }
    const rootEnd = checkedAdd(
      BigInt(HEADER_BYTES),
      header.rootLength,
      "root end",
    );
    if (rootEnd > header.dataOffset || header.dataOffset > sourceSize) {
      throw corrupt("archive root/directory offsets are inconsistent");
    }
    if (rootEnd > BigInt(bootstrap.byteLength)) {
      const remainder = await source.read(
        BigInt(bootstrap.byteLength),
        safeNumber(rootEnd - BigInt(bootstrap.byteLength), "root remainder"),
        signalOptions(options.signal),
      );
      const combined = new Uint8Array(
        bootstrap.byteLength + remainder.byteLength,
      );
      combined.set(bootstrap);
      combined.set(remainder, bootstrap.byteLength);
      bootstrap = combined;
    }
    const manifests = decodeRoot(
      bootstrap.slice(HEADER_BYTES, safeNumber(rootEnd, "root end")),
      header,
    );
    return new ArchiveReader(
      source,
      sourceSize,
      bootstrap,
      header,
      manifests,
      options,
    );
  } catch (error) {
    await source.close?.();
    throw error;
  }
}
