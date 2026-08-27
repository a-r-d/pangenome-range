import { blake3 } from "@noble/hashes/blake3.js";
import { decompress as fzstdDecompress } from "fzstd";
import { assembleCanonicalGraph, canonicalGraphHash } from "./canonical.js";
import {
  ARCHIVE_METADATA_TYPE_ID,
  type DecodedArchiveMetadata,
  decodeArchiveMetadata,
  decodeLocusPage,
  decodeNamedLociDescriptor,
  decodeSummaryDescriptor,
  decodeSummaryPage,
  FeatureDecodeError,
  NAMED_LOCI_TYPE_ID,
  type NamedLociDescriptor,
  normalizeLocusKey,
  SUMMARY_PYRAMID_TYPE_ID,
  type SummaryPyramidDescriptor,
  selectLocusPages,
  selectSummarySeries,
} from "./features.js";
import { decodeRegionalPayload } from "./regional.js";
import { BlobRangeSource, HttpRangeSource } from "./sources.js";
import type {
  ArchiveCacheStats,
  ArchiveCapabilities,
  ArchiveInfo,
  ArchiveProvenance,
  ChunkDecompressor,
  FeatureQueryTrace,
  FeatureRequestRange,
  LocusHit,
  LocusSearch,
  LocusSearchResult,
  OpenPangenomeOptions,
  OverviewBin,
  PangenomeArchive,
  QueryRequestRange,
  QueryTrace,
  RangeReadOptions,
  RangeSource,
  ReferenceDescriptor,
  RegionPlan,
  RegionQuery,
  RegionResult,
  RegionTile,
  SummaryQuery,
  SummaryResult,
} from "./types.js";

const ARCHIVE_MAGIC = "PNGRNG01";
const ARCHIVE_VERSION = 1;
const HEADER_BYTES = 64;
const BOOTSTRAP_BYTES = 16 * 1024;
const DIRECTORY_PAGE_BYTES = 4 * 1024;
const DIRECTORY_ENTRY_BYTES = 56;
const DIRECTORY_ENTRY_CAPACITY = 72;
const DEFAULT_DIRECTORY_CACHE_BYTES = 1024 * 1024;
const DEFAULT_PAYLOAD_CACHE_BYTES = 32 * 1024 * 1024;
const DEFAULT_EXTENSION_CACHE_BYTES = 8 * 1024 * 1024;
const DEFAULT_DECODED_FEATURE_CACHE_BYTES = 16 * 1024 * 1024;
const FEATURE_SEARCH_CONCURRENCY = 4;
const DEFAULT_MAX_ROOT_BYTES = 16 * 1024 * 1024;
const MAX_EXTENSION_DIRECTORY_BYTES = 1024 * 1024;
const DEFAULT_MAX_CHUNK_BYTES = 64 * 1024 * 1024;
const DEFAULT_PAYLOAD_COALESCING_GAP_BYTES = 64 * 1024;
const CONSTRUCTION_CONTEXT = 100;
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
  version: number;
  rootLength: bigint;
  entryCount: bigint;
  dataOffset: bigint;
  extensionDirectoryOffset: bigint;
  extensionDirectoryLength: bigint;
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
  integrity: Uint8Array;
}

interface ExtensionEntry {
  typeId: Uint8Array;
  required: boolean;
  codec: ChunkCodec;
  offset: bigint;
  encodedLength: bigint;
  decodedLength: bigint;
  integrity: Uint8Array;
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

  get bytes(): number {
    return this.#bytes;
  }

  get entries(): number {
    return this.#values.size;
  }

  clear(): void {
    this.#values.clear();
    this.#bytes = 0;
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

interface MutableQueryTrace {
  requestRanges: QueryRequestRange[];
  dependencyRounds: number;
  bootstrapHits: number;
  directoryHits: number;
  payloadHits: number;
  integrityMs: number;
  integrityIntervals: TimeInterval[];
  decompressionTaskMs: number;
  decompressionIntervals: TimeInterval[];
  decodeMs: number;
  decodeIntervals: TimeInterval[];
  directoryRoundRecorded: boolean;
  payloadRoundRecorded: boolean;
}

interface MutableFeatureTrace {
  requestRanges: FeatureRequestRange[];
  dependencyRounds: number;
  requestedLayers: Set<FeatureRequestRange["layer"]>;
  cacheHits: number;
  integrityMs: number;
  integrityIntervals: TimeInterval[];
  decompressionTaskMs: number;
  decompressionIntervals: TimeInterval[];
  decodeMs: number;
  decodeIntervals: TimeInterval[];
  pagesAvoidedByLimit: number;
}

interface TimeInterval {
  readonly start: number;
  readonly end: number;
}

function featureTraceState(): MutableFeatureTrace {
  return {
    requestRanges: [],
    dependencyRounds: 0,
    requestedLayers: new Set(),
    cacheHits: 0,
    integrityMs: 0,
    integrityIntervals: [],
    decompressionTaskMs: 0,
    decompressionIntervals: [],
    decodeMs: 0,
    decodeIntervals: [],
    pagesAvoidedByLimit: 0,
  };
}

function finishFeatureTrace(state: MutableFeatureTrace): FeatureQueryTrace {
  return {
    dependencyRounds: state.dependencyRounds,
    requestRanges: state.requestRanges,
    totalBytes: state.requestRanges.reduce(
      (total, range) => total + range.length,
      0,
    ),
    cacheHits: state.cacheHits,
    integrityMs: intervalUnionMs(state.integrityIntervals),
    decompressionMs: exclusiveIntervalUnionMs(state.decompressionIntervals, [
      ...state.decodeIntervals,
      ...state.integrityIntervals,
    ]),
    decompressionTaskMs: state.decompressionTaskMs,
    decodeMs: intervalUnionMs(state.decodeIntervals),
    pagesAvoidedByLimit: state.pagesAvoidedByLimit,
  };
}

function traceState(
  openRanges: readonly QueryRequestRange[],
  openDependencyRounds: number,
): MutableQueryTrace {
  return {
    requestRanges: [...openRanges],
    dependencyRounds: openDependencyRounds,
    bootstrapHits: 1,
    directoryHits: 0,
    payloadHits: 0,
    integrityMs: 0,
    integrityIntervals: [],
    decompressionTaskMs: 0,
    decompressionIntervals: [],
    decodeMs: 0,
    decodeIntervals: [],
    directoryRoundRecorded: false,
    payloadRoundRecorded: false,
  };
}

function traceBytes(ranges: readonly QueryRequestRange[]): {
  total: number;
  unique: number;
} {
  const total = ranges.reduce((sum, range) => sum + range.length, 0);
  const intervals = ranges
    .filter(({ length }) => length > 0)
    .map(({ offset, length }) => ({
      start: offset,
      end: offset + BigInt(length),
    }))
    .sort((left, right) =>
      left.start < right.start ? -1 : left.start > right.start ? 1 : 0,
    );
  let unique = 0n;
  let currentStart: bigint | undefined;
  let currentEnd: bigint | undefined;
  for (const interval of intervals) {
    if (currentStart === undefined || currentEnd === undefined) {
      currentStart = interval.start;
      currentEnd = interval.end;
    } else if (interval.start <= currentEnd) {
      if (interval.end > currentEnd) currentEnd = interval.end;
    } else {
      unique += currentEnd - currentStart;
      currentStart = interval.start;
      currentEnd = interval.end;
    }
  }
  if (currentStart !== undefined && currentEnd !== undefined) {
    unique += currentEnd - currentStart;
  }
  return { total, unique: safeNumber(unique, "query trace unique bytes") };
}

function finishTrace(
  state: MutableQueryTrace,
  mergeMs: number,
  canonicalHash: string,
  tiles: readonly RegionTile[],
  selectedNodes: number,
): QueryTrace {
  const bytes = traceBytes(state.requestRanges);
  const layerBytes = (layer: QueryRequestRange["layer"]): number =>
    state.requestRanges
      .filter((range) => range.layer === layer)
      .reduce((total, range) => total + range.length, 0);
  return {
    dependencyRounds: state.dependencyRounds,
    requestRanges: state.requestRanges,
    totalBytes: bytes.total,
    uniqueBytes: bytes.unique,
    duplicateBytes: bytes.total - bytes.unique,
    bootstrapBytes: layerBytes("bootstrap"),
    directoryBytes: layerBytes("directory"),
    payloadBytes: layerBytes("payload"),
    cacheHits: {
      bootstrap: state.bootstrapHits,
      directory: state.directoryHits,
      payload: state.payloadHits,
    },
    decompressionMs: exclusiveIntervalUnionMs(state.decompressionIntervals, [
      ...state.decodeIntervals,
      ...state.integrityIntervals,
    ]),
    decompressionTaskMs: state.decompressionTaskMs,
    integrityMs: intervalUnionMs(state.integrityIntervals),
    decodeMs: intervalUnionMs(state.decodeIntervals),
    mergeMs,
    selectedChunks: tiles.length,
    selectedNodes,
    selectedTraversals: tiles.reduce(
      (total, tile) => total + tile.haplotypes.weights.length,
      0,
    ),
    canonicalHash,
  };
}

function recordDecompressionInterval(
  trace: MutableQueryTrace | MutableFeatureTrace,
  start: number,
  end: number,
): void {
  trace.decompressionTaskMs += end - start;
  trace.decompressionIntervals.push({ start, end });
}

async function decompressAndTrace(
  decompressor: ChunkDecompressor,
  compressed: Uint8Array,
  expectedLength: number,
  options?: RangeReadOptions,
  trace?: MutableQueryTrace | MutableFeatureTrace,
): Promise<Uint8Array> {
  const started = performance.now();
  const pending = decompressor.decompress(compressed, expectedLength, options);
  if (pending instanceof Uint8Array) {
    if (trace !== undefined) {
      recordDecompressionInterval(trace, started, performance.now());
    }
    return pending;
  }
  try {
    return await pending;
  } finally {
    if (trace !== undefined) {
      recordDecompressionInterval(trace, started, performance.now());
    }
  }
}

function recordIntegrityInterval(
  trace: MutableQueryTrace | MutableFeatureTrace,
  start: number,
  end: number,
): void {
  trace.integrityMs += end - start;
  trace.integrityIntervals.push({ start, end });
}

function recordDecodeInterval(
  trace: MutableQueryTrace | MutableFeatureTrace,
  start: number,
  end: number,
): void {
  trace.decodeMs += end - start;
  trace.decodeIntervals.push({ start, end });
}

function intervalUnionMs(intervals: readonly TimeInterval[]): number {
  return mergedTimeIntervals(intervals).reduce(
    (total, interval) => total + interval.end - interval.start,
    0,
  );
}

function exclusiveIntervalUnionMs(
  intervals: readonly TimeInterval[],
  blockers: readonly TimeInterval[],
): number {
  const active = mergedTimeIntervals(intervals);
  const excluded = mergedTimeIntervals(blockers);
  let total = 0;
  for (const interval of active) {
    let cursor = interval.start;
    for (const blocker of excluded) {
      if (blocker.end <= cursor) continue;
      if (blocker.start >= interval.end) break;
      total += Math.max(0, Math.min(blocker.start, interval.end) - cursor);
      cursor = Math.max(cursor, blocker.end);
      if (cursor >= interval.end) break;
    }
    total += Math.max(0, interval.end - cursor);
  }
  return total;
}

function mergedTimeIntervals(
  intervals: readonly TimeInterval[],
): TimeInterval[] {
  const sorted = [...intervals]
    .filter((interval) => interval.end >= interval.start)
    .sort((left, right) => left.start - right.start);
  const merged: TimeInterval[] = [];
  for (const interval of sorted) {
    const previous = merged.at(-1);
    if (previous === undefined || interval.start > previous.end) {
      merged.push({ ...interval });
    } else if (interval.end > previous.end) {
      merged[merged.length - 1] = { start: previous.start, end: interval.end };
    }
  }
  return merged;
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
  const extensionDirectoryOffset = reader.u64();
  const extensionDirectoryLength = reader.u64();
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
  if (
    (extensionDirectoryOffset === 0n) !== (extensionDirectoryLength === 0n) ||
    extensionDirectoryLength > BigInt(MAX_EXTENSION_DIRECTORY_BYTES)
  ) {
    throw corrupt("invalid extension directory pointer");
  }
  if (
    extensionDirectoryLength !== 0n &&
    extensionDirectoryOffset !==
      checkedAdd(BigInt(HEADER_BYTES), rootLength, "root end")
  ) {
    throw corrupt("invalid extension directory pointer");
  }
  return {
    version,
    rootLength,
    entryCount,
    dataOffset,
    extensionDirectoryOffset,
    extensionDirectoryLength,
  };
}

function directoryStart(header: Header): bigint {
  return header.extensionDirectoryLength === 0n
    ? checkedAdd(BigInt(HEADER_BYTES), header.rootLength, "root end")
    : checkedAdd(
        header.extensionDirectoryOffset,
        header.extensionDirectoryLength,
        "extension directory end",
      );
}

function codecLabel(
  codec: ChunkCodec,
): "none" | "zstd-1" | "zstd-3" | "zstd-6" {
  switch (codec) {
    case 0:
      return "none";
    case 1:
      return "zstd-1";
    case 3:
      return "zstd-3";
    case 6:
      return "zstd-6";
  }
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
  let previousPageEnd = directoryStart(header);
  let totalEntries = 0n;
  const identities = new Set<string>();
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
    const identity = `${manifest.sample.length}:${manifest.sample}${manifest.contig.length}:${manifest.contig}:${manifest.start}:${manifest.end}`;
    if (identities.has(identity)) {
      throw corrupt("duplicate reference manifest interval");
    }
    identities.add(identity);
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

function compareBytes(left: Uint8Array, right: Uint8Array): number {
  for (let index = 0; index < left.byteLength; index += 1) {
    const difference = (left[index] as number) - (right[index] as number);
    if (difference !== 0) return difference;
  }
  return left.byteLength - right.byteLength;
}

function decodeExtensionDirectory(
  bytes: Uint8Array,
  header: Header,
  sourceSize: bigint,
): ExtensionEntry[] {
  const reader = new BinaryReader(bytes);
  if (
    textDecoder.decode(reader.take(8)) !== "PNGEXT01" ||
    reader.u32() !== 1 ||
    reader.u32() !== 64
  ) {
    throw corrupt("invalid extension directory header");
  }
  const count = safeNumber(reader.u64(), "extension entry count");
  assertZero(reader.take(8), "extension directory reserved bytes");
  if (count > Math.floor(reader.remaining / 64)) {
    throw corrupt("extension entry count exceeds directory bytes");
  }
  let previousType: Uint8Array | undefined;
  const entries: ExtensionEntry[] = [];
  for (let index = 0; index < count; index += 1) {
    const typeId = reader.take(16).slice();
    const flags = reader.u32();
    const codec = decodeCodec(reader.u8());
    assertZero(reader.take(3), "extension entry reserved bytes");
    const offset = reader.u64();
    const encodedLength = reader.u64();
    const decodedLength = reader.u64();
    const integrity = reader.take(16).slice();
    const end = checkedAdd(offset, encodedLength, "extension payload end");
    if (
      typeId.every((byte) => byte === 0) ||
      (previousType !== undefined && compareBytes(typeId, previousType) <= 0) ||
      (flags & ~1) !== 0 ||
      encodedLength === 0n ||
      decodedLength === 0n ||
      offset < header.dataOffset ||
      end > sourceSize
    ) {
      throw corrupt("invalid extension entry");
    }
    const known =
      typeIdText(typeId) === ARCHIVE_METADATA_TYPE_ID ||
      typeIdText(typeId) === NAMED_LOCI_TYPE_ID ||
      typeIdText(typeId) === SUMMARY_PYRAMID_TYPE_ID;
    if ((flags & 1) !== 0 && !known) {
      throw corrupt("archive contains an unknown required extension");
    }
    previousType = typeId;
    entries.push({
      typeId,
      required: (flags & 1) !== 0,
      codec,
      offset,
      encodedLength,
      decodedLength,
      integrity,
    });
  }
  reader.finish();
  return entries;
}

function typeIdText(typeId: Uint8Array): string {
  try {
    return textDecoder.decode(typeId);
  } catch {
    return "";
  }
}

function decodeFeature<T>(decode: () => T): T {
  try {
    return decode();
  } catch (error) {
    if (error instanceof FeatureDecodeError) throw corrupt(error.message);
    throw error;
  }
}

function validateSummaryDescriptor(
  descriptor: SummaryPyramidDescriptor,
  manifests: readonly Manifest[],
): void {
  const nextLevels = manifests.map(() => 0);
  const nextSpans = manifests.map(() => descriptor.baseBinSpan);
  const finalCounts = manifests.map(() => 0n);
  for (const series of descriptor.series) {
    const manifest = manifests[series.manifestIndex];
    const expectedLevel = nextLevels[series.manifestIndex];
    const expectedSpan = nextSpans[series.manifestIndex];
    if (
      manifest === undefined ||
      expectedLevel === undefined ||
      expectedSpan === undefined ||
      series.level !== expectedLevel ||
      series.binSpan !== expectedSpan
    ) {
      throw corrupt("summary series level or manifest is not canonical");
    }
    const first = (manifest.start / series.binSpan) * series.binSpan;
    const last = ((manifest.end - 1n) / series.binSpan) * series.binSpan;
    const count = (last - first) / series.binSpan + 1n;
    if (series.firstBinStart !== first || series.binCount !== count) {
      throw corrupt("summary series dimensions are not canonical");
    }
    nextLevels[series.manifestIndex] = expectedLevel + 1;
    nextSpans[series.manifestIndex] = checkedMultiply(
      expectedSpan,
      4n,
      "summary level span",
    );
    finalCounts[series.manifestIndex] = count;
  }
  if (
    nextLevels.some((count) => count === 0) ||
    finalCounts.some((count) => count !== 1n)
  ) {
    throw corrupt(
      "summary pyramid does not cover every reference through one top bin",
    );
  }
}

function validateSummaryQuery(query: SummaryQuery): void {
  if (query.sample.length === 0 || query.contig.length === 0) {
    throw new TypeError("summary sample and contig must not be empty");
  }
  for (const [value, label] of [
    [query.start, "summary start"],
    [query.end, "summary end"],
  ] as const) {
    if (!Number.isSafeInteger(value) || value < 0) {
      throw new RangeError(`${label} must be a non-negative safe integer`);
    }
  }
  if (query.end <= query.start) {
    throw new RangeError("summary end must be greater than start");
  }
  const maxBins = query.maxBins ?? 512;
  if (!Number.isSafeInteger(maxBins) || maxBins < 1 || maxBins > 4096) {
    throw new RangeError("summary maxBins must be in 1..=4096");
  }
}

function hex(bytes: Uint8Array): string {
  return [...bytes].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

function archiveProvenance(
  metadata: DecodedArchiveMetadata,
): ArchiveProvenance {
  return {
    sourceGbzBytes: metadata.sourceGbzBytes,
    sourceGbzSha256: hex(metadata.sourceGbzSha256),
    encoderPackageVersion: metadata.encoderPackageVersion,
    formatImplementation: metadata.formatImplementation,
    regionalWindowSize: safeNumber(
      metadata.regionalWindowSize,
      "regional window size",
    ),
    constructionContext: safeNumber(
      metadata.constructionContext,
      "construction context",
    ),
    payloadCodec: codecLabel(metadata.payloadCodec),
    haplotypeSemantics: "anonymous-distinct-weighted-tile-paths",
    ...(metadata.referenceSample === undefined
      ? {}
      : { referenceSample: metadata.referenceSample }),
    ...(metadata.referenceAssembly === undefined
      ? {}
      : { referenceAssembly: metadata.referenceAssembly }),
    ...(metadata.datasetTitle === undefined
      ? {}
      : { datasetTitle: metadata.datasetTitle }),
    ...(metadata.datasetDescription === undefined
      ? {}
      : { datasetDescription: metadata.datasetDescription }),
    ...(metadata.sourceUri === undefined
      ? {}
      : { sourceUri: metadata.sourceUri }),
    ...(metadata.annotationFilename === undefined
      ? {}
      : { annotationFilename: metadata.annotationFilename }),
    ...(metadata.annotationSha256 === undefined
      ? {}
      : { annotationSha256: hex(metadata.annotationSha256) }),
    ...(metadata.annotationRelease === undefined
      ? {}
      : { annotationRelease: metadata.annotationRelease }),
    ...(metadata.annotationAssembly === undefined
      ? {}
      : { annotationAssembly: metadata.annotationAssembly }),
  };
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
  let previousKey: readonly bigint[] | undefined;
  for (let index = 0; index < count; index += 1) {
    const start = reader.u64();
    const end = reader.u64();
    const offset = reader.u64();
    const compressedLength = reader.u64();
    const uncompressedLength = reader.u64();
    const integrity = reader.take(16);
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
    const integrityHigh = new DataView(
      integrity.buffer,
      integrity.byteOffset,
      integrity.byteLength,
    ).getBigUint64(0, true);
    const integrityLow = new DataView(
      integrity.buffer,
      integrity.byteOffset,
      integrity.byteLength,
    ).getBigUint64(8, true);
    const key = [
      start,
      end,
      offset,
      compressedLength,
      uncompressedLength,
      integrityHigh,
      integrityLow,
    ];
    if (previousKey !== undefined) {
      for (let keyIndex = 0; keyIndex < key.length; keyIndex += 1) {
        const value = key[keyIndex] as bigint;
        const previous = previousKey[keyIndex] as bigint;
        if (value < previous) {
          throw corrupt("directory entries are not ordered");
        }
        if (value > previous) break;
      }
    }
    previousKey = key;
    entries.push({
      manifest,
      start,
      end,
      offset,
      compressedLength,
      uncompressedLength,
      integrity,
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
    if (query.context > CONSTRUCTION_CONTEXT) {
      throw new RangeError(
        `query.context exceeds the construction halo ${CONSTRUCTION_CONTEXT}`,
      );
    }
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
    const frame = zstdFrameMetadata(compressed);
    const declaredLength = frame.contentSize;
    if (frame.encodedLength !== compressed.byteLength) {
      throw corrupt("zstd payload must contain exactly one frame");
    }
    if (declaredLength !== BigInt(expectedLength)) {
      throw corrupt(
        `zstd frame declares ${declaredLength} bytes, expected ${expectedLength}`,
      );
    }
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

function zstdFrameMetadata(compressed: Uint8Array): {
  contentSize: bigint;
  encodedLength: number;
} {
  if (
    compressed.byteLength < 6 ||
    compressed[0] !== 0x28 ||
    compressed[1] !== 0xb5 ||
    compressed[2] !== 0x2f ||
    compressed[3] !== 0xfd
  ) {
    throw corrupt("zstd payload has no standard frame header");
  }
  const descriptor = compressed[4] as number;
  if ((descriptor & 0x18) !== 0) {
    throw corrupt("zstd frame uses reserved descriptor bits");
  }
  if ((descriptor & 0x04) !== 0) {
    throw corrupt(
      "zstd content-checksum frames are not supported in file-format v1",
    );
  }
  const singleSegment = (descriptor & 0x20) !== 0;
  const dictionaryFlag = descriptor & 0x03;
  if (dictionaryFlag !== 0) {
    throw corrupt("zstd dictionaries are not supported");
  }
  const dictionaryBytes = [0, 1, 2, 4][dictionaryFlag] as number;
  const sizeFlag = descriptor >>> 6;
  const contentSizeBytes =
    sizeFlag === 0
      ? singleSegment
        ? 1
        : 0
      : sizeFlag === 1
        ? 2
        : sizeFlag === 2
          ? 4
          : 8;
  if (contentSizeBytes === 0) {
    throw corrupt("zstd frame omits its decompressed content size");
  }
  const contentSizeOffset = 5 + (singleSegment ? 0 : 1) + dictionaryBytes;
  if (contentSizeOffset + contentSizeBytes > compressed.byteLength) {
    throw corrupt("zstd frame header is truncated");
  }
  let value = 0n;
  for (let index = 0; index < contentSizeBytes; index += 1) {
    value +=
      BigInt(compressed[contentSizeOffset + index] as number) <<
      BigInt(index * 8);
  }
  const contentSize = contentSizeBytes === 2 ? value + 256n : value;
  let position = contentSizeOffset + contentSizeBytes;
  let lastBlock = false;
  while (!lastBlock) {
    if (position + 3 > compressed.byteLength) {
      throw corrupt("zstd block header is truncated");
    }
    const blockHeader =
      (compressed[position] as number) |
      ((compressed[position + 1] as number) << 8) |
      ((compressed[position + 2] as number) << 16);
    position += 3;
    lastBlock = (blockHeader & 1) !== 0;
    const blockType = (blockHeader >>> 1) & 0x03;
    const blockSize = blockHeader >>> 3;
    if (blockType === 3) throw corrupt("zstd frame uses a reserved block type");
    const encodedBlockSize = blockType === 1 ? 1 : blockSize;
    if (position + encodedBlockSize > compressed.byteLength) {
      throw corrupt("zstd block is truncated");
    }
    position += encodedBlockSize;
  }
  return { contentSize, encodedLength: position };
}

class ArchiveReader implements PangenomeArchive {
  readonly formatVersion: number;
  readonly semantics: "anonymous-distinct-weighted-tile-paths";
  readonly #source: RangeSource;
  readonly #sourceSize: bigint;
  readonly #bootstrap: Uint8Array;
  readonly #header: Header;
  readonly #manifests: Manifest[];
  readonly #extensions: ExtensionEntry[];
  readonly #directoryCache: ByteCache;
  readonly #payloadCache: ByteCache;
  readonly #extensionCache: ByteCache;
  readonly #decodedFeatureCache: ByteCache;
  readonly #decompressor: ChunkDecompressor;
  readonly #maxChunkBytes: number;
  readonly #payloadCoalescingGapBytes: number;
  readonly #openRanges: readonly QueryRequestRange[];
  readonly #openDependencyRounds: number;
  #namedLociDescriptor: NamedLociDescriptor | undefined;
  #summaryDescriptor: SummaryPyramidDescriptor | undefined;
  #archiveMetadata: DecodedArchiveMetadata | undefined;
  #closed = false;

  constructor(
    source: RangeSource,
    sourceSize: bigint,
    bootstrap: Uint8Array,
    header: Header,
    manifests: Manifest[],
    extensions: ExtensionEntry[],
    options: OpenPangenomeOptions,
    openRanges: readonly QueryRequestRange[],
    openDependencyRounds: number,
  ) {
    this.formatVersion = header.version;
    this.semantics = "anonymous-distinct-weighted-tile-paths";
    this.#source = source;
    this.#sourceSize = sourceSize;
    this.#bootstrap = bootstrap;
    this.#header = header;
    this.#manifests = manifests;
    this.#extensions = extensions;
    this.#directoryCache = new ByteCache(
      options.directoryCacheBytes ?? DEFAULT_DIRECTORY_CACHE_BYTES,
    );
    this.#payloadCache = new ByteCache(
      options.payloadCacheBytes ?? DEFAULT_PAYLOAD_CACHE_BYTES,
    );
    this.#extensionCache = new ByteCache(
      options.extensionCacheBytes ?? DEFAULT_EXTENSION_CACHE_BYTES,
    );
    this.#decodedFeatureCache = new ByteCache(
      options.decodedFeatureCacheBytes ?? DEFAULT_DECODED_FEATURE_CACHE_BYTES,
    );
    this.#decompressor = options.decompressor ?? new FzstdDecompressor();
    this.#maxChunkBytes = options.maxChunkBytes ?? DEFAULT_MAX_CHUNK_BYTES;
    this.#payloadCoalescingGapBytes =
      options.payloadCoalescingGapBytes ?? DEFAULT_PAYLOAD_COALESCING_GAP_BYTES;
    this.#openRanges = openRanges;
    this.#openDependencyRounds = openDependencyRounds;
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

  capabilities(): ArchiveCapabilities {
    this.#assertOpen();
    return {
      namedLoci: this.#extension(NAMED_LOCI_TYPE_ID) !== undefined,
      multiscaleSummaries:
        this.#extension(SUMMARY_PYRAMID_TYPE_ID) !== undefined,
    };
  }

  async info(options: { signal?: AbortSignal } = {}): Promise<ArchiveInfo> {
    this.#assertOpen();
    options.signal?.throwIfAborted();
    const namedEntry = this.#extension(NAMED_LOCI_TYPE_ID);
    const summaryEntry = this.#extension(SUMMARY_PYRAMID_TYPE_ID);
    const metadataEntry = this.#extension(ARCHIVE_METADATA_TYPE_ID);
    const [named, summaries, metadata] = await Promise.all([
      namedEntry === undefined
        ? undefined
        : this.#loadNamedLociDescriptor(options.signal),
      summaryEntry === undefined
        ? undefined
        : this.#loadSummaryDescriptor(options.signal),
      metadataEntry === undefined
        ? undefined
        : this.#loadArchiveMetadata(options.signal),
    ]);
    const levelsByManifest = summaries?.series.reduce<number[]>(
      (levels, series) => {
        levels[series.manifestIndex] = Math.max(
          levels[series.manifestIndex] ?? 0,
          series.level + 1,
        );
        return levels;
      },
      this.#manifests.map(() => 0),
    );
    const strongRemoteIdentity = this.#source.strongIdentity?.();
    return {
      formatVersion: this.formatVersion,
      haplotypeSemantics: this.semantics,
      archiveBytes: this.#sourceSize,
      ...(strongRemoteIdentity === undefined ? {} : { strongRemoteIdentity }),
      references: this.references(),
      extensions: this.#extensions.map((entry) => typeIdText(entry.typeId)),
      namedLoci: {
        state:
          named === undefined
            ? "absent"
            : named.recordCount === 0n
              ? "present-empty"
              : "present-populated",
        recordCount: named?.recordCount ?? 0n,
      },
      ...(summaries === undefined || levelsByManifest === undefined
        ? {}
        : {
            summaries: {
              baseBinSpan: safeNumber(
                summaries.baseBinSpan,
                "summary base bin span",
              ),
              levelsByManifest,
            },
          }),
      ...(metadata === undefined
        ? {}
        : { provenance: archiveProvenance(metadata) }),
    };
  }

  async searchLoci(query: LocusSearch): Promise<LocusSearchResult> {
    this.#assertOpen();
    query.signal?.throwIfAborted();
    const normalizedQuery = normalizeLocusKey(query.name);
    if (normalizedQuery.length === 0) {
      throw new TypeError("locus search name must not be empty");
    }
    const mode = query.mode ?? "exact";
    if (mode !== "exact" && mode !== "prefix") {
      throw new TypeError("locus search mode must be exact or prefix");
    }
    const limit = query.limit ?? 50;
    if (!Number.isSafeInteger(limit) || limit < 1 || limit > 1000) {
      throw new RangeError("locus search limit must be in 1..=1000");
    }
    const instrument = query.trace !== undefined && query.trace !== false;
    const trace = instrument ? featureTraceState() : undefined;
    const descriptor = await this.#loadNamedLociDescriptor(query.signal, trace);
    const candidates = selectLocusPages(descriptor, normalizedQuery, mode);
    const decodePage = async (page: (typeof candidates)[number]) => {
      const raw = await this.#readFeatureDecoded(
        page.storage,
        "extension-page",
        query.signal,
        trace,
      );
      const started = performance.now();
      const records = decodeFeature(() => decodeLocusPage(raw));
      if (
        BigInt(records.length) !== page.recordCount ||
        records.at(0)?.normalizedKey !== page.firstKey ||
        records.at(-1)?.normalizedKey !== page.lastKey
      ) {
        throw corrupt("named-locus page differs from its descriptor");
      }
      if (trace !== undefined) {
        recordDecodeInterval(trace, started, performance.now());
      }
      return records;
    };
    const hits: LocusHit[] = [];
    let truncated = false;
    let fetchedPages = 0;
    outer: while (fetchedPages < candidates.length) {
      const batch = candidates.slice(
        fetchedPages,
        fetchedPages + FEATURE_SEARCH_CONCURRENCY,
      );
      const decoded = await Promise.all(batch.map(decodePage));
      fetchedPages += batch.length;
      for (const records of decoded) {
        for (const record of records) {
          const nameMatches =
            mode === "exact"
              ? record.normalizedKey === normalizedQuery
              : record.normalizedKey.startsWith(normalizedQuery);
          if (
            !nameMatches ||
            (query.sample !== undefined && record.sample !== query.sample) ||
            (query.contig !== undefined && record.contig !== query.contig)
          ) {
            continue;
          }
          if (hits.length === limit) {
            truncated = true;
            break outer;
          }
          hits.push({
            matchedName: record.matchedName,
            displayName: record.displayName,
            stableId: record.stableId,
            featureType: record.featureType,
            reference: {
              sample: record.sample,
              contig: record.contig,
              start: safeNumber(record.start, "locus start"),
              end: safeNumber(record.end, "locus end"),
              orientation: "forward",
            },
            strand:
              record.strand === 1
                ? "forward"
                : record.strand === 2
                  ? "reverse"
                  : "unknown",
          });
        }
      }
    }
    if (trace !== undefined && truncated) {
      trace.pagesAvoidedByLimit = candidates.length - fetchedPages;
    }
    const completedTrace = trace && finishFeatureTrace(trace);
    if (typeof query.trace === "function" && completedTrace !== undefined) {
      query.trace(completedTrace);
    }
    const checksumPresent = descriptor.annotationSha256.some(
      (value) => value !== 0,
    );
    return {
      query: query.name,
      normalizedQuery,
      mode,
      ...(descriptor.annotationName.length === 0
        ? {}
        : { annotationName: descriptor.annotationName }),
      ...(checksumPresent
        ? { annotationSha256: hex(descriptor.annotationSha256) }
        : {}),
      totalIndexedRecords: descriptor.recordCount,
      hits,
      truncated,
      ...(completedTrace === undefined ? {} : { trace: completedTrace }),
    };
  }

  async summary(query: SummaryQuery): Promise<SummaryResult> {
    this.#assertOpen();
    validateSummaryQuery(query);
    query.signal?.throwIfAborted();
    const maxBins = query.maxBins ?? 512;
    const trace =
      query.trace !== undefined && query.trace !== false
        ? featureTraceState()
        : undefined;
    const descriptor = await this.#loadSummaryDescriptor(query.signal, trace);
    const queryStart = BigInt(query.start);
    const queryEnd = BigInt(query.end);
    const manifestIndexes = this.#manifests
      .map((manifest, index) => ({ manifest, index }))
      .filter(
        ({ manifest }) =>
          manifest.sample === query.sample &&
          manifest.contig === query.contig &&
          manifest.start < queryEnd &&
          manifest.end > queryStart,
      );
    if (manifestIndexes.length === 0) {
      throw new RangeError(
        `archive has no reference interval for ${query.sample}#${query.contig}:${query.start}-${query.end}`,
      );
    }
    const selected = decodeFeature(() =>
      selectSummarySeries(
        descriptor,
        manifestIndexes.map(({ index }) => index),
        queryStart,
        queryEnd,
        maxBins,
      ),
    );
    const pages = await Promise.all(
      selected.map(async (series) => {
        const raw = await this.#readFeatureDecoded(
          series.storage,
          "extension-page",
          query.signal,
          trace,
        );
        const started = performance.now();
        const bins = decodeFeature(() => decodeSummaryPage(raw, series));
        if (trace !== undefined) {
          recordDecodeInterval(trace, started, performance.now());
        }
        return { series, bins };
      }),
    );
    const bins: OverviewBin[] = [];
    for (const { series, bins: seriesBins } of pages) {
      const manifest = this.#manifests[series.manifestIndex];
      if (manifest === undefined) throw corrupt("summary manifest is missing");
      for (let index = 0; index < seriesBins.length; index += 1) {
        const binStart = checkedAdd(
          series.firstBinStart,
          checkedMultiply(BigInt(index), series.binSpan, "summary bin start"),
          "summary bin start",
        );
        const binEnd = checkedAdd(binStart, series.binSpan, "summary bin end");
        if (binStart >= queryEnd || binEnd <= queryStart) continue;
        const value = seriesBins[index];
        if (value === undefined) throw corrupt("summary bin is missing");
        const fullBinStart =
          binStart > manifest.start ? binStart : manifest.start;
        const fullBinEnd = binEnd < manifest.end ? binEnd : manifest.end;
        const clippedStart =
          fullBinStart > queryStart ? fullBinStart : queryStart;
        const clippedEnd = fullBinEnd < queryEnd ? fullBinEnd : queryEnd;
        bins.push({
          reference: {
            sample: manifest.sample,
            contig: manifest.contig,
            start: safeNumber(clippedStart, "summary bin start"),
            end: safeNumber(clippedEnd, "summary bin end"),
            orientation: "forward",
          },
          fullBinStart: safeNumber(fullBinStart, "summary full bin start"),
          fullBinEnd: safeNumber(fullBinEnd, "summary full bin end"),
          coverageFraction:
            Number(clippedEnd - clippedStart) /
            Number(fullBinEnd - fullBinStart),
          level: series.level,
          binSpan: safeNumber(series.binSpan, "summary bin span"),
          ...value,
        });
      }
    }
    bins.sort(
      (left, right) =>
        left.reference.start - right.reference.start ||
        left.reference.end - right.reference.end,
    );
    const completedTrace = trace && finishFeatureTrace(trace);
    if (typeof query.trace === "function" && completedTrace !== undefined) {
      query.trace(completedTrace);
    }
    return {
      query: {
        sample: query.sample,
        contig: query.contig,
        start: query.start,
        end: query.end,
        ...(query.maxBins === undefined ? {} : { maxBins: query.maxBins }),
      },
      bins,
      ...(completedTrace === undefined ? {} : { trace: completedTrace }),
    };
  }

  async planRegion(query: RegionQuery): Promise<RegionPlan> {
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
    const ranges = [...uniqueEntries.values()]
      .sort((left, right) =>
        left.start < right.start
          ? -1
          : left.start > right.start
            ? 1
            : left.end < right.end
              ? -1
              : left.end > right.end
                ? 1
                : 0,
      )
      .map((entry) => ({
        coreStart: safeNumber(entry.start, "planned core start"),
        coreEnd: safeNumber(entry.end, "planned core end"),
        offset: entry.offset,
        compressedBytes: entry.compressedLength,
        decodedBytes: entry.uncompressedLength,
      }));
    return {
      sample: query.sample,
      contig: query.contig,
      start: query.start,
      end: query.end,
      selectedChunks: ranges.length,
      compressedBytes: ranges.reduce(
        (total, range) => total + range.compressedBytes,
        0n,
      ),
      decodedBytes: ranges.reduce(
        (total, range) => total + range.decodedBytes,
        0n,
      ),
      ranges,
    };
  }

  async query(query: RegionQuery): Promise<RegionResult> {
    this.#assertOpen();
    validateQuery(query);
    const instrument = query.trace !== undefined && query.trace !== false;
    const trace = instrument
      ? traceState(this.#openRanges, this.#openDependencyRounds)
      : undefined;
    const tiles: RegionTile[] = [];
    for await (const tile of this.#streamTiles(query, trace)) tiles.push(tile);
    tiles.sort(
      (left, right) => left.start - right.start || left.end - right.end,
    );
    const mergeStarted = performance.now();
    const graph = assembleCanonicalGraph(tiles, query);
    const mergeMs = performance.now() - mergeStarted;
    const baseResult: RegionResult = {
      query: {
        sample: query.sample,
        contig: query.contig,
        start: query.start,
        end: query.end,
        ...(query.context === undefined ? {} : { context: query.context }),
      },
      semantics: this.semantics,
      graph,
      tiles,
    };
    if (trace !== undefined) {
      const completed = finishTrace(
        trace,
        mergeMs,
        canonicalGraphHash(graph),
        tiles,
        graph.nodes.ids.length,
      );
      if (typeof query.trace === "function") query.trace(completed);
      return { ...baseResult, trace: completed };
    }
    return baseResult;
  }

  async *queryTiles(query: RegionQuery): AsyncIterable<RegionTile> {
    this.#assertOpen();
    validateQuery(query);
    const instrument = typeof query.trace === "function";
    const trace = instrument
      ? traceState(this.#openRanges, this.#openDependencyRounds)
      : undefined;
    const tiles: RegionTile[] = [];
    for await (const tile of this.#streamTiles(query, trace)) {
      tiles.push(tile);
      yield tile;
    }
    if (trace !== undefined && typeof query.trace === "function") {
      const mergeStarted = performance.now();
      const graph = assembleCanonicalGraph(tiles, query);
      const mergeMs = performance.now() - mergeStarted;
      query.trace(
        finishTrace(
          trace,
          mergeMs,
          canonicalGraphHash(graph),
          tiles,
          graph.nodes.ids.length,
        ),
      );
    }
  }

  async *#streamTiles(
    query: RegionQuery,
    trace?: MutableQueryTrace,
  ): AsyncIterable<RegionTile> {
    const entries = await this.#lookup(query, trace);
    const uniqueEntries = new Map<string, DirectoryEntry>();
    for (const entry of entries) {
      uniqueEntries.set(
        `${entry.offset}:${entry.compressedLength}:${entry.uncompressedLength}`,
        entry,
      );
    }
    const tilePromises = await this.#prepareTiles(
      [...uniqueEntries.values()],
      query.signal,
      trace,
    );
    const pending = tilePromises.map((promise, index) => ({
      index,
      promise: promise.then((tile) => ({ index, tile })),
    }));
    while (pending.length > 0) {
      const completed = await Promise.race(pending.map((item) => item.promise));
      const index = pending.findIndex((item) => item.index === completed.index);
      pending.splice(index, 1);
      yield completed.tile;
    }
  }

  cacheStats(): ArchiveCacheStats {
    this.#assertOpen();
    return {
      directoryBytes: this.#directoryCache.bytes,
      directoryEntries: this.#directoryCache.entries,
      payloadBytes: this.#payloadCache.bytes,
      payloadEntries: this.#payloadCache.entries,
      extensionBytes: this.#extensionCache.bytes,
      extensionEntries: this.#extensionCache.entries,
      decodedFeatureBytes: this.#decodedFeatureCache.bytes,
      decodedFeatureEntries: this.#decodedFeatureCache.entries,
    };
  }

  clearCaches(): void {
    this.#assertOpen();
    this.#directoryCache.clear();
    this.#payloadCache.clear();
    this.#extensionCache.clear();
    this.#decodedFeatureCache.clear();
    this.#namedLociDescriptor = undefined;
    this.#summaryDescriptor = undefined;
    this.#archiveMetadata = undefined;
  }

  async close(): Promise<void> {
    if (!this.#closed) {
      this.#closed = true;
      await this.#source.close?.();
    }
  }

  #extension(typeId: string): ExtensionEntry | undefined {
    return this.#extensions.find(
      (entry) => typeIdText(entry.typeId) === typeId,
    );
  }

  async #loadNamedLociDescriptor(
    signal?: AbortSignal,
    trace?: MutableFeatureTrace,
  ): Promise<NamedLociDescriptor> {
    if (this.#namedLociDescriptor !== undefined) {
      return this.#namedLociDescriptor;
    }
    const entry = this.#extension(NAMED_LOCI_TYPE_ID);
    if (entry === undefined) {
      throw new RangeError("archive does not contain a named-locus index");
    }
    const raw = await this.#readFeatureDecoded(
      entry,
      "extension-descriptor",
      signal,
      trace,
    );
    const started = performance.now();
    const descriptor = decodeFeature(() =>
      decodeNamedLociDescriptor(raw, this.#header.dataOffset, this.#sourceSize),
    );
    if (trace !== undefined) {
      recordDecodeInterval(trace, started, performance.now());
    }
    this.#namedLociDescriptor = descriptor;
    return descriptor;
  }

  async #loadArchiveMetadata(
    signal?: AbortSignal,
  ): Promise<DecodedArchiveMetadata> {
    if (this.#archiveMetadata !== undefined) return this.#archiveMetadata;
    const entry = this.#extension(ARCHIVE_METADATA_TYPE_ID);
    if (entry === undefined) {
      throw new RangeError("archive does not contain provenance metadata");
    }
    const raw = await this.#readFeatureDecoded(
      entry,
      "extension-descriptor",
      signal,
    );
    const metadata = decodeFeature(() => decodeArchiveMetadata(raw));
    this.#archiveMetadata = metadata;
    return metadata;
  }

  async #loadSummaryDescriptor(
    signal?: AbortSignal,
    trace?: MutableFeatureTrace,
  ): Promise<SummaryPyramidDescriptor> {
    if (this.#summaryDescriptor !== undefined) return this.#summaryDescriptor;
    const entry = this.#extension(SUMMARY_PYRAMID_TYPE_ID);
    if (entry === undefined) {
      throw new RangeError(
        "archive does not contain a multiscale summary pyramid",
      );
    }
    const raw = await this.#readFeatureDecoded(
      entry,
      "extension-descriptor",
      signal,
      trace,
    );
    const started = performance.now();
    const descriptor = decodeFeature(() =>
      decodeSummaryDescriptor(raw, this.#header.dataOffset, this.#sourceSize),
    );
    validateSummaryDescriptor(descriptor, this.#manifests);
    if (trace !== undefined) {
      recordDecodeInterval(trace, started, performance.now());
    }
    this.#summaryDescriptor = descriptor;
    return descriptor;
  }

  async #readFeatureDecoded(
    storage: {
      offset: bigint;
      encodedLength: bigint;
      decodedLength: bigint;
      codec: ChunkCodec;
      integrity: Uint8Array;
    },
    layer: FeatureRequestRange["layer"],
    signal?: AbortSignal,
    trace?: MutableFeatureTrace,
  ): Promise<Uint8Array> {
    const key = `${storage.offset}:${storage.encodedLength}`;
    const decodedKey = `${key}:${storage.decodedLength}:${storage.codec}`;
    const cachedRaw = this.#decodedFeatureCache.get(decodedKey);
    if (cachedRaw !== undefined) {
      if (trace !== undefined) trace.cacheHits += 1;
      return cachedRaw;
    }
    let encoded = this.#extensionCache.get(key);
    if (encoded === undefined) {
      const length = safeNumber(
        storage.encodedLength,
        "extension encoded length",
      );
      const end = checkedAdd(
        storage.offset,
        storage.encodedLength,
        "extension range end",
      );
      if (end > this.#sourceSize)
        throw corrupt("extension range is outside the archive");
      const bootstrapEnd = BigInt(this.#bootstrap.byteLength);
      if (end <= bootstrapEnd) {
        encoded = this.#bootstrap.slice(
          safeNumber(storage.offset, "extension bootstrap offset"),
          safeNumber(end, "extension bootstrap end"),
        );
      } else if (storage.offset >= bootstrapEnd) {
        this.#recordFeatureRequest(storage.offset, length, layer, trace);
        encoded = await this.#source.read(
          storage.offset,
          length,
          signalOptions(signal),
        );
      } else {
        const prefix = this.#bootstrap.slice(
          safeNumber(storage.offset, "extension bootstrap offset"),
        );
        const suffixLength = safeNumber(
          end - bootstrapEnd,
          "extension suffix length",
        );
        this.#recordFeatureRequest(bootstrapEnd, suffixLength, layer, trace);
        const suffix = await this.#source.read(
          bootstrapEnd,
          suffixLength,
          signalOptions(signal),
        );
        encoded = new Uint8Array(length);
        encoded.set(prefix);
        encoded.set(suffix, prefix.byteLength);
      }
      const integrityStarted = performance.now();
      const actual = blake3(encoded).subarray(0, 16);
      if (
        actual.some(
          (byte, index) => byte !== (storage.integrity[index] as number),
        )
      ) {
        throw corrupt("extension payload integrity mismatch");
      }
      if (trace !== undefined) {
        recordIntegrityInterval(trace, integrityStarted, performance.now());
      }
      this.#extensionCache.set(key, encoded);
    } else if (trace !== undefined) {
      trace.cacheHits += 1;
    }
    const expectedLength = safeNumber(
      storage.decodedLength,
      "extension decoded length",
    );
    const raw =
      storage.codec === 0
        ? encoded.slice()
        : await decompressAndTrace(
            this.#decompressor,
            encoded,
            expectedLength,
            signalOptions(signal),
            trace,
          );
    if (raw.byteLength !== expectedLength) {
      throw corrupt("extension decoded length mismatch");
    }
    this.#decodedFeatureCache.set(decodedKey, raw);
    return raw;
  }

  #recordFeatureRequest(
    offset: bigint,
    length: number,
    layer: FeatureRequestRange["layer"],
    trace?: MutableFeatureTrace,
  ): void {
    if (trace === undefined) return;
    trace.requestRanges.push({ offset, length, layer });
    if (!trace.requestedLayers.has(layer)) {
      trace.requestedLayers.add(layer);
      trace.dependencyRounds += 1;
    }
  }

  async #lookup(
    query: RegionQuery,
    trace?: MutableQueryTrace,
  ): Promise<DirectoryEntry[]> {
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
          trace,
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
    trace?: MutableQueryTrace,
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
    const missingIndexes: number[] = [];
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
      if (cached === undefined) {
        missingIndexes.push(index);
      } else if (trace !== undefined) {
        trace.directoryHits += 1;
      }
      pages.push({ bytes: cached ?? new Uint8Array(), bucketIndex });
    }
    if (missingIndexes.length === 0) return pages;
    const spans: Array<{ first: number; last: number }> = [];
    for (const index of missingIndexes) {
      const previous = spans.at(-1);
      if (previous !== undefined && index === previous.last + 1) {
        previous.last = index;
      } else {
        spans.push({ first: index, last: index });
      }
    }
    const storedSpans = spans.map(({ first, last }) => {
      const bucketIndex = firstBucket + BigInt(first);
      const offset = checkedAdd(
        manifest.firstPageOffset,
        checkedMultiply(
          bucketIndex,
          BigInt(DIRECTORY_PAGE_BYTES),
          "page span start",
        ),
        "page span start",
      );
      return {
        first,
        last,
        offset,
        length: (last - first + 1) * DIRECTORY_PAGE_BYTES,
      };
    });
    if (
      trace !== undefined &&
      storedSpans.some(
        ({ offset, length }) =>
          offset + BigInt(length) > BigInt(this.#bootstrap.byteLength),
      ) &&
      !trace.directoryRoundRecorded
    ) {
      trace.dependencyRounds += 1;
      trace.directoryRoundRecorded = true;
    }
    const fetchedSpans = await Promise.all(
      storedSpans.map(async (span) => ({
        ...span,
        bytes: await this.#readStored(
          span.offset,
          span.length,
          signal,
          "directory",
          trace,
        ),
      })),
    );
    for (const span of fetchedSpans) {
      for (let index = span.first; index <= span.last; index += 1) {
        const start = (index - span.first) * DIRECTORY_PAGE_BYTES;
        const page = span.bytes.slice(start, start + DIRECTORY_PAGE_BYTES);
        const offset = span.offset + BigInt(start);
        this.#directoryCache.set(String(offset), page);
        pages[index] = {
          bytes: page,
          bucketIndex: firstBucket + BigInt(index),
        };
      }
    }
    return pages;
  }

  async #prepareTiles(
    entries: readonly DirectoryEntry[],
    signal?: AbortSignal,
    trace?: MutableQueryTrace,
  ): Promise<Array<Promise<RegionTile>>> {
    signal?.throwIfAborted();
    const compressedByKey = new Map<string, Uint8Array>();
    const missingByKey = new Map<string, DirectoryEntry>();
    for (const entry of entries) {
      const key = `${entry.offset}:${entry.compressedLength}`;
      const cached = this.#payloadCache.get(key);
      if (cached === undefined) {
        missingByKey.set(key, entry);
      } else {
        compressedByKey.set(key, cached);
        if (trace !== undefined) trace.payloadHits += 1;
      }
    }
    const missing = [...missingByKey.values()];

    const ranges: Array<{ start: bigint; end: bigint }> = [];
    for (const entry of [...missing].sort((left, right) =>
      left.offset < right.offset ? -1 : left.offset > right.offset ? 1 : 0,
    )) {
      const end = checkedAdd(
        entry.offset,
        entry.compressedLength,
        "payload end",
      );
      const previous = ranges.at(-1);
      const mergedLength = previous === undefined ? 0n : end - previous.start;
      if (
        previous !== undefined &&
        entry.offset <=
          previous.end + BigInt(this.#payloadCoalescingGapBytes) &&
        mergedLength <= BigInt(this.#maxChunkBytes)
      ) {
        if (end > previous.end) previous.end = end;
      } else {
        ranges.push({ start: entry.offset, end });
      }
    }
    if (
      trace !== undefined &&
      ranges.some(({ end }) => end > BigInt(this.#bootstrap.byteLength)) &&
      !trace.payloadRoundRecorded
    ) {
      trace.dependencyRounds += 1;
      trace.payloadRoundRecorded = true;
    }
    const fetched = await Promise.all(
      ranges.map(async (range) => ({
        range,
        bytes: await this.#readStored(
          range.start,
          safeNumber(range.end - range.start, "coalesced payload range"),
          signal,
          "payload",
          trace,
        ),
      })),
    );
    for (const entry of missing) {
      const entryEnd = entry.offset + entry.compressedLength;
      const stored = fetched.find(
        ({ range }) => range.start <= entry.offset && range.end >= entryEnd,
      );
      if (stored === undefined) {
        throw corrupt("coalesced payload response does not cover its chunk");
      }
      const start = safeNumber(
        entry.offset - stored.range.start,
        "payload slice start",
      );
      const length = safeNumber(
        entry.compressedLength,
        "compressed payload length",
      );
      const compressed = stored.bytes.slice(start, start + length);
      const key = `${entry.offset}:${entry.compressedLength}`;
      this.#verifyPayloadIntegrity(entry, compressed, trace);
      compressedByKey.set(key, compressed);
      this.#payloadCache.set(key, compressed);
    }
    return entries.map((entry) => {
      const key = `${entry.offset}:${entry.compressedLength}`;
      const compressed = compressedByKey.get(key);
      if (compressed === undefined) {
        throw corrupt("payload cache lost a selected chunk");
      }
      return this.#decodeTile(entry, compressed, signal, trace);
    });
  }

  async #decodeTile(
    entry: DirectoryEntry,
    compressed: Uint8Array,
    signal?: AbortSignal,
    trace?: MutableQueryTrace,
  ): Promise<RegionTile> {
    const expectedLength = safeNumber(
      entry.uncompressedLength,
      "uncompressed payload length",
    );
    const raw =
      entry.manifest.codec === 0
        ? compressed.slice()
        : await decompressAndTrace(
            this.#decompressor,
            compressed,
            expectedLength,
            signalOptions(signal),
            trace,
          );
    signal?.throwIfAborted();
    if (raw.byteLength !== expectedLength) {
      throw corrupt(
        `regional payload has ${raw.byteLength} bytes after decompression, expected ${expectedLength}`,
      );
    }
    const decodeStarted = performance.now();
    const tile = decodeRegionalPayload(raw, {
      archiveOffset: entry.offset,
      encodedLength: safeNumber(
        entry.compressedLength,
        "encoded payload length",
      ),
      codec: codecLabel(entry.manifest.codec),
      coreStart: safeNumber(entry.start, "regional core start"),
      coreEnd: safeNumber(entry.end, "regional core end"),
      referenceSample: entry.manifest.sample,
      referenceContig: entry.manifest.contig,
    });
    if (trace !== undefined) {
      recordDecodeInterval(trace, decodeStarted, performance.now());
    }
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

  #verifyPayloadIntegrity(
    entry: DirectoryEntry,
    compressed: Uint8Array,
    trace?: MutableQueryTrace,
  ): void {
    const started = performance.now();
    const actual = blake3(compressed).subarray(0, 16);
    if (
      actual.some((byte, index) => byte !== (entry.integrity[index] as number))
    ) {
      throw corrupt("regional payload integrity mismatch");
    }
    if (trace !== undefined) {
      recordIntegrityInterval(trace, started, performance.now());
    }
  }

  async #readStored(
    offset: bigint,
    length: number,
    signal?: AbortSignal,
    layer: QueryRequestRange["layer"] = "payload",
    trace?: MutableQueryTrace,
  ): Promise<Uint8Array> {
    assertNonNegativeSafeInteger(length, "stored range length");
    const end = checkedAdd(offset, BigInt(length), "stored range end");
    if (end > this.#sourceSize) {
      throw corrupt("stored range is outside the archive");
    }
    const bootstrapEnd = BigInt(this.#bootstrap.byteLength);
    if (end <= bootstrapEnd) {
      if (trace !== undefined) trace.bootstrapHits += 1;
      return this.#bootstrap.slice(
        safeNumber(offset, "bootstrap range offset"),
        safeNumber(end, "bootstrap range end"),
      );
    }
    if (offset >= bootstrapEnd) {
      trace?.requestRanges.push({ offset, length, layer });
      return this.#source.read(offset, length, signalOptions(signal));
    }
    if (trace !== undefined) trace.bootstrapHits += 1;
    const prefix = this.#bootstrap.slice(
      safeNumber(offset, "bootstrap range offset"),
    );
    const suffixLength = safeNumber(end - bootstrapEnd, "stored range suffix");
    trace?.requestRanges.push({
      offset: bootstrapEnd,
      length: suffixLength,
      layer,
    });
    const suffix = await this.#source.read(
      bootstrapEnd,
      suffixLength,
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
    return new HttpRangeSource(options.source, {
      ...(options.fetch === undefined ? {} : { fetch: options.fetch }),
      ...(options.httpHeaders === undefined
        ? {}
        : { headers: options.httpHeaders }),
      ...(options.httpCache === undefined ? {} : { cache: options.httpCache }),
      ...(options.httpUseHead === undefined
        ? {}
        : { useHead: options.httpUseHead }),
      ...(options.httpUseIfRange === undefined
        ? {}
        : { useIfRange: options.httpUseIfRange }),
      ...(options.maxFullResponseBytes === undefined
        ? {}
        : { maxFullResponseBytes: options.maxFullResponseBytes }),
    });
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
  const extensionCacheBytes =
    options.extensionCacheBytes ?? DEFAULT_EXTENSION_CACHE_BYTES;
  const decodedFeatureCacheBytes =
    options.decodedFeatureCacheBytes ?? DEFAULT_DECODED_FEATURE_CACHE_BYTES;
  const maxRootBytes = options.maxRootBytes ?? DEFAULT_MAX_ROOT_BYTES;
  const maxChunkBytes = options.maxChunkBytes ?? DEFAULT_MAX_CHUNK_BYTES;
  const payloadCoalescingGapBytes =
    options.payloadCoalescingGapBytes ?? DEFAULT_PAYLOAD_COALESCING_GAP_BYTES;
  for (const [value, label] of [
    [directoryCacheBytes, "directoryCacheBytes"],
    [payloadCacheBytes, "payloadCacheBytes"],
    [extensionCacheBytes, "extensionCacheBytes"],
    [decodedFeatureCacheBytes, "decodedFeatureCacheBytes"],
    [maxRootBytes, "maxRootBytes"],
    [maxChunkBytes, "maxChunkBytes"],
    [payloadCoalescingGapBytes, "payloadCoalescingGapBytes"],
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
    const openRanges: QueryRequestRange[] = [
      { offset: 0n, length: firstLength, layer: "bootstrap" },
    ];
    let openDependencyRounds = 1;
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
    const metadataEnd = directoryStart(header);
    if (
      rootEnd > metadataEnd ||
      metadataEnd > header.dataOffset ||
      header.dataOffset > sourceSize
    ) {
      throw corrupt("archive root/directory offsets are inconsistent");
    }
    if (metadataEnd > BigInt(bootstrap.byteLength)) {
      const remainder = await source.read(
        BigInt(bootstrap.byteLength),
        safeNumber(
          metadataEnd - BigInt(bootstrap.byteLength),
          "metadata remainder",
        ),
        signalOptions(options.signal),
      );
      openRanges.push({
        offset: BigInt(bootstrap.byteLength),
        length: remainder.byteLength,
        layer: "bootstrap",
      });
      openDependencyRounds += 1;
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
    const extensions =
      header.extensionDirectoryLength === 0n
        ? []
        : decodeExtensionDirectory(
            bootstrap.slice(
              safeNumber(
                header.extensionDirectoryOffset,
                "extension directory offset",
              ),
              safeNumber(metadataEnd, "extension directory end"),
            ),
            header,
            sourceSize,
          );
    return new ArchiveReader(
      source,
      sourceSize,
      bootstrap,
      header,
      manifests,
      extensions,
      options,
      openRanges,
      openDependencyRounds,
    );
  } catch (error) {
    await source.close?.();
    throw error;
  }
}
