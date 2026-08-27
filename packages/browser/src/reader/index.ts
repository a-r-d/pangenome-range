export {
  CorruptArchiveError,
  FzstdDecompressor,
  UnsupportedArchiveVersionError,
  UnsupportedChunkCodecError,
} from "./archive.js";
export {
  canonicalGraphHash,
  canonicalHaplotypeTileHash,
} from "./canonical.js";
export type { DecodeRegionalPayloadOptions } from "./regional.js";
export {
  CorruptRegionalPayloadError,
  decodeRegionalPayload,
  detectRegionalPayloadVersion,
  UnsupportedRegionalPayloadVersionError,
} from "./regional.js";
export type {
  HttpRangeRequest,
  HttpRangeSourceOptions,
  TracedRangeRead,
  TracedRangeSummary,
} from "./sources.js";
export {
  BlobRangeSource,
  HttpRangeResponseError,
  HttpRangeSource,
  MemoryRangeSource,
  RemoteObjectChangedError,
  TracingRangeSource,
} from "./sources.js";
export type {
  ArchiveCacheStats,
  ArchiveCapabilities,
  ArchiveInfo,
  ArchiveProvenance,
  ChunkDecompressor,
  EdgeTable,
  FeatureQueryTrace,
  FeatureRequestRange,
  HaplotypeSemantics,
  LocusHit,
  LocusSearch,
  LocusSearchResult,
  NodeTable,
  OpenPangenomeInput,
  OpenPangenomeOptions,
  OverviewBin,
  PangenomeArchive,
  QueryCacheHits,
  QueryRequestRange,
  QueryTrace,
  RangeReadOptions,
  RangeSource,
  ReferenceDescriptor,
  RegionGraph,
  RegionPlan,
  RegionPlanRange,
  RegionQuery,
  RegionResult,
  RegionTile,
  SummaryQuery,
  SummaryResult,
  TileProvenance,
  WeightedTraversalTable,
} from "./types.js";

import {
  openPangenomeArchive,
  UnsupportedArchiveVersionError,
} from "./archive.js";
import type {
  OpenPangenomeInput,
  OpenPangenomeOptions,
  PangenomeArchive,
  RegionQuery,
} from "./types.js";

export const PANGENOME_RANGE_API_VERSION = "0.1.0" as const;
const CONSTRUCTION_CONTEXT = 100;

const textDecoder = new TextDecoder();

/** Dispatches the versioned archive header without truncating any offsets. */
export function detectArchiveVersion(header: Uint8Array): 1 {
  if (header.byteLength < 12) {
    throw new RangeError("archive header is shorter than 12 bytes");
  }
  const magic = textDecoder.decode(header.subarray(0, 8));
  const version = new DataView(
    header.buffer,
    header.byteOffset,
    header.byteLength,
  ).getUint32(8, true);
  if (magic === "PNGRNG01" && version === 1) {
    return 1;
  }
  throw new UnsupportedArchiveVersionError(
    `unsupported pangenome-range archive magic ${JSON.stringify(magic)} version ${version}`,
  );
}

function assertSafeNonNegativeInteger(value: number, label: string): void {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new RangeError(`${label} must be a non-negative safe integer`);
  }
}

export function validateRegionQuery(query: RegionQuery): void {
  if (query.sample.length === 0) {
    throw new TypeError("query.sample must not be empty");
  }
  if (query.contig.length === 0) {
    throw new TypeError("query.contig must not be empty");
  }
  assertSafeNonNegativeInteger(query.start, "query.start");
  assertSafeNonNegativeInteger(query.end, "query.end");
  if (query.end <= query.start) {
    throw new RangeError("query.end must be greater than query.start");
  }
  if (query.context !== undefined) {
    assertSafeNonNegativeInteger(query.context, "query.context");
    if (query.context > CONSTRUCTION_CONTEXT) {
      throw new RangeError(
        `query.context exceeds the construction halo ${CONSTRUCTION_CONTEXT}`,
      );
    }
  }
}

export function validateArchiveRange(offset: bigint, length: number): void {
  if (offset < 0n) {
    throw new RangeError("archive offset must be non-negative");
  }
  assertSafeNonNegativeInteger(length, "range length");
}

function validateOptionalCacheSize(
  value: number | undefined,
  label: string,
): void {
  if (value !== undefined) {
    assertSafeNonNegativeInteger(value, label);
  }
}

export function openPangenome(
  input: OpenPangenomeInput,
): Promise<PangenomeArchive> {
  const options: OpenPangenomeOptions =
    typeof input === "object" &&
    input !== null &&
    "source" in input &&
    !("read" in input && typeof input.read === "function") &&
    !(input instanceof Blob)
      ? (input as OpenPangenomeOptions)
      : { source: input as string | Blob | OpenPangenomeOptions["source"] };
  options.signal?.throwIfAborted();
  validateOptionalCacheSize(options.directoryCacheBytes, "directoryCacheBytes");
  validateOptionalCacheSize(options.payloadCacheBytes, "payloadCacheBytes");
  validateOptionalCacheSize(options.extensionCacheBytes, "extensionCacheBytes");

  return openPangenomeArchive(options);
}
