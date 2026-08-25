/** Options shared by exact byte-range reads. */
export interface RangeReadOptions {
  signal?: AbortSignal;
}

/**
 * A source that provides exact byte ranges.
 *
 * Raw archive offsets are always bigint. They must never be converted to a
 * JavaScript number before addressing the source.
 */
export interface RangeSource {
  size(signal?: AbortSignal): Promise<bigint>;
  read(
    offset: bigint,
    length: number,
    options?: RangeReadOptions,
  ): Promise<Uint8Array>;
  close?(): void | Promise<void>;
}

export interface ChunkDecompressor {
  decompress(
    compressed: Uint8Array,
    expectedLength: number,
    options?: RangeReadOptions,
  ): Uint8Array | Promise<Uint8Array>;
}

/**
 * Reference coordinates are numbers only after safe-integer validation.
 * Raw archive byte offsets remain bigint throughout the reader.
 */
export interface RegionQuery {
  sample: string;
  contig: string;
  start: number;
  end: number;
  context?: number;
  signal?: AbortSignal;
}

export interface OpenPangenomeOptions {
  source: string | Blob | RangeSource;
  fetch?: typeof globalThis.fetch;
  directoryCacheBytes?: number;
  payloadCacheBytes?: number;
  decompressor?: ChunkDecompressor;
  signal?: AbortSignal;
}

export interface ReferenceDescriptor {
  readonly sample: string;
  readonly contig: string;
  readonly start: number;
  readonly end: number;
  readonly fragment?: number;
  readonly orientation?: "forward" | "reverse";
}

export type HaplotypeSemantics =
  | "named-paths-v3"
  | "anonymous-all-tile-paths"
  | "anonymous-distinct-weighted-tile-paths";

/** A decoded tile. Numeric graph fields are typed-array-oriented. */
export interface RegionTile {
  readonly reference: ReferenceDescriptor;
  readonly start: number;
  readonly end: number;
  readonly semantics: HaplotypeSemantics;
  readonly archiveOffset: bigint;
  readonly encodedLength: number;
  readonly nodeIds: BigUint64Array;
  readonly edges: BigUint64Array;
  readonly traversalOffsets: Uint32Array;
  readonly traversalNodes: BigUint64Array;
  readonly traversalWeights: BigUint64Array;
}

export interface RegionResult {
  readonly query: Readonly<RegionQuery>;
  readonly semantics: HaplotypeSemantics;
  readonly tiles: readonly RegionTile[];
}

export interface PangenomeArchive {
  readonly formatVersion: number;
  references(): readonly ReferenceDescriptor[];
  query(query: RegionQuery): Promise<RegionResult>;
  queryTiles(query: RegionQuery): AsyncIterable<RegionTile>;
  close(): void | Promise<void>;
}
