/** Options shared by exact byte-range reads. */
export interface RangeReadOptions {
  signal?: AbortSignal;
  /** Override the source's normal HTTP cache mode for an explicit benchmark. */
  cache?: RequestCache;
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
  /** Enable optional query instrumentation or receive it when streaming ends. */
  trace?: boolean | ((trace: QueryTrace) => void);
}

export interface OpenPangenomeOptions {
  source: string | Blob | RangeSource;
  fetch?: typeof globalThis.fetch;
  httpHeaders?: HeadersInit;
  httpCache?: RequestCache;
  httpUseHead?: boolean;
  httpUseIfRange?: boolean;
  maxFullResponseBytes?: number;
  directoryCacheBytes?: number;
  payloadCacheBytes?: number;
  payloadCoalescingGapBytes?: number;
  maxRootBytes?: number;
  maxChunkBytes?: number;
  decompressor?: ChunkDecompressor;
  signal?: AbortSignal;
}

export interface ReferenceDescriptor {
  readonly sample: string;
  readonly contig: string;
  readonly start: number;
  readonly end: number;
  readonly fragment?: number;
  readonly haplotype?: number;
  readonly orientation?: "forward" | "reverse";
}

export type HaplotypeSemantics = "anonymous-distinct-weighted-tile-paths";

export interface NodeTable {
  readonly ids: BigUint64Array;
  readonly sequenceOffsets: Uint32Array;
  readonly sequenceBytes: Uint8Array;
}

export interface EdgeTable {
  readonly from: BigUint64Array;
  readonly to: BigUint64Array;
}

export interface WeightedTraversalTable {
  readonly kind: "weighted-traversals";
  readonly traversalOffsets: Uint32Array;
  readonly orientedNodes: BigUint64Array;
  readonly weights: BigUint64Array;
}

export interface TileProvenance {
  readonly archiveOffset: bigint;
  readonly compressedBytes: number;
  readonly uncompressedBytes: number;
  readonly codec: "none" | "zstd-1" | "zstd-3" | "zstd-6";
}

/** A decoded tile. Numeric graph fields are typed-array-oriented. */
export interface RegionTile {
  readonly reference: ReferenceDescriptor;
  readonly coreStart: number;
  readonly coreEnd: number;
  /** Compatibility aliases for the initial private reader API. */
  readonly start: number;
  readonly end: number;
  readonly semantics: HaplotypeSemantics;
  readonly nodes: NodeTable;
  readonly topology: EdgeTable;
  readonly haplotypes: WeightedTraversalTable;
  readonly provenance: TileProvenance;
  /** Flat table aliases retained for the initial private reader API. */
  readonly archiveOffset: bigint;
  readonly encodedLength: number;
  readonly nodeIds: BigUint64Array;
  readonly nodeSequenceOffsets: Uint32Array;
  readonly nodeSequences: Uint8Array;
  readonly edges: BigUint64Array;
  readonly referenceTraversal: BigUint64Array;
  readonly traversalOffsets: Uint32Array;
  readonly traversalNodes: BigUint64Array;
  readonly traversalWeights: BigUint64Array;
}

export interface RegionGraph {
  readonly reference: ReferenceDescriptor;
  readonly nodes: NodeTable;
  readonly edges: EdgeTable;
  readonly referenceTraversal: BigUint64Array;
}

export interface QueryRequestRange {
  readonly offset: bigint;
  readonly length: number;
  readonly layer: "bootstrap" | "directory" | "payload";
}

export interface QueryCacheHits {
  readonly bootstrap: number;
  readonly directory: number;
  readonly payload: number;
}

export interface QueryTrace {
  readonly dependencyRounds: number;
  readonly requestRanges: readonly QueryRequestRange[];
  readonly totalBytes: number;
  readonly uniqueBytes: number;
  readonly duplicateBytes: number;
  readonly bootstrapBytes: number;
  readonly directoryBytes: number;
  readonly payloadBytes: number;
  readonly cacheHits: QueryCacheHits;
  readonly integrityMs: number;
  readonly decompressionMs: number;
  readonly decodeMs: number;
  readonly mergeMs: number;
  readonly selectedChunks: number;
  readonly selectedNodes: number;
  readonly selectedTraversals: number;
  readonly canonicalHash: string;
}

export interface RegionResult {
  readonly query: Readonly<RegionQuery>;
  readonly semantics: HaplotypeSemantics;
  readonly graph: RegionGraph;
  readonly tiles: readonly RegionTile[];
  readonly trace?: QueryTrace;
}

export interface ArchiveCacheStats {
  readonly directoryBytes: number;
  readonly directoryEntries: number;
  readonly payloadBytes: number;
  readonly payloadEntries: number;
}

export interface PangenomeArchive {
  readonly formatVersion: number;
  readonly semantics: HaplotypeSemantics;
  references(): readonly ReferenceDescriptor[];
  query(query: RegionQuery): Promise<RegionResult>;
  queryTiles(query: RegionQuery): AsyncIterable<RegionTile>;
  cacheStats(): ArchiveCacheStats;
  clearCaches(): void;
  close(): void | Promise<void>;
}

export type OpenPangenomeInput =
  | string
  | Blob
  | RangeSource
  | OpenPangenomeOptions;
