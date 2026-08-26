export type { DecodeRegionalPayloadOptions } from "./regional.js";
export {
  CorruptRegionalPayloadError,
  decodeRegionalPayload,
  detectRegionalPayloadVersion,
  UnsupportedRegionalPayloadVersionError,
} from "./regional.js";
export type {
  ChunkDecompressor,
  HaplotypeSemantics,
  OpenPangenomeOptions,
  PangenomeArchive,
  RangeReadOptions,
  RangeSource,
  ReferenceDescriptor,
  RegionQuery,
  RegionResult,
  RegionTile,
} from "./types.js";

import type {
  OpenPangenomeOptions,
  PangenomeArchive,
  RegionQuery,
} from "./types.js";

export const PANGENOME_RANGE_API_VERSION = "0.1.0" as const;

export class NotImplementedError extends Error {
  override readonly name = "NotImplementedError";
}

export class UnsupportedArchiveVersionError extends Error {
  override readonly name = "UnsupportedArchiveVersionError";
}

const textDecoder = new TextDecoder();

/** Dispatches the versioned archive header without truncating any offsets. */
export function detectArchiveVersion(header: Uint8Array): 4 {
  if (header.byteLength < 12) {
    throw new RangeError("archive header is shorter than 12 bytes");
  }
  const magic = textDecoder.decode(header.subarray(0, 8));
  const version = new DataView(
    header.buffer,
    header.byteOffset,
    header.byteLength,
  ).getUint32(8, true);
  if (magic === "PNGRNG04" && version === 4) {
    return 4;
  }
  if (magic === "PNGRNG03" && version === 3) {
    throw new UnsupportedArchiveVersionError(
      "archive v3 uses legacy named-path semantics and is not supported by the TypeScript reader",
    );
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
  options: OpenPangenomeOptions,
): Promise<PangenomeArchive> {
  options.signal?.throwIfAborted();
  validateOptionalCacheSize(options.directoryCacheBytes, "directoryCacheBytes");
  validateOptionalCacheSize(options.payloadCacheBytes, "payloadCacheBytes");

  return Promise.reject(
    new NotImplementedError(
      "The pangenome-range archive decoder is not implemented in this scaffolding release",
    ),
  );
}
