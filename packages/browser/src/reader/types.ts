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
  /** Strong immutable identity, when the source can expose one. */
  strongIdentity?(): string | undefined;
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
  extensionCacheBytes?: number;
  decodedFeatureCacheBytes?: number;
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
  /** Wall time covered by the union of decompression task intervals. */
  readonly decompressionMs: number;
  /** Sum of decompression task durations; may exceed elapsed wall time. */
  readonly decompressionTaskMs: number;
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
  readonly extensionBytes: number;
  readonly extensionEntries: number;
  readonly decodedFeatureBytes: number;
  readonly decodedFeatureEntries: number;
}

export interface ArchiveCapabilities {
  readonly namedLoci: boolean;
  readonly multiscaleSummaries: boolean;
}

export interface ArchiveProvenance {
  readonly sourceGbzBytes: bigint;
  readonly sourceGbzSha256: string;
  readonly encoderPackageVersion: string;
  readonly formatImplementation: string;
  readonly regionalWindowSize: number;
  readonly constructionContext: number;
  readonly payloadCodec: "none" | "zstd-1" | "zstd-3" | "zstd-6";
  readonly haplotypeSemantics: HaplotypeSemantics;
  readonly referenceSample?: string;
  readonly referenceAssembly?: string;
  readonly datasetTitle?: string;
  readonly datasetDescription?: string;
  readonly sourceUri?: string;
  readonly annotationFilename?: string;
  readonly annotationSha256?: string;
  readonly annotationRelease?: string;
  readonly annotationAssembly?: string;
}

export interface ArchiveInfo {
  readonly formatVersion: number;
  readonly haplotypeSemantics: HaplotypeSemantics;
  readonly archiveBytes: bigint;
  readonly strongRemoteIdentity?: string;
  readonly references: readonly ReferenceDescriptor[];
  readonly extensions: readonly string[];
  readonly namedLoci: {
    readonly state: "absent" | "present-empty" | "present-populated";
    readonly recordCount: bigint;
  };
  readonly summaries?: {
    readonly baseBinSpan: number;
    readonly levelsByManifest: readonly number[];
  };
  readonly provenance?: ArchiveProvenance;
}

export interface FeatureRequestRange {
  readonly offset: bigint;
  readonly length: number;
  readonly layer: "extension-descriptor" | "extension-page";
}

export interface FeatureQueryTrace {
  readonly dependencyRounds: number;
  readonly requestRanges: readonly FeatureRequestRange[];
  readonly totalBytes: number;
  readonly cacheHits: number;
  readonly pagesAvoidedByLimit: number;
  readonly integrityMs: number;
  /** Wall time covered by the union of decompression task intervals. */
  readonly decompressionMs: number;
  /** Sum of decompression task durations; may exceed elapsed wall time. */
  readonly decompressionTaskMs: number;
  readonly decodeMs: number;
}

export interface LocusSearch {
  readonly name: string;
  readonly mode?: "exact" | "prefix";
  readonly sample?: string;
  readonly contig?: string;
  readonly limit?: number;
  readonly signal?: AbortSignal;
  readonly trace?: boolean | ((trace: FeatureQueryTrace) => void);
}

export interface LocusHit {
  readonly matchedName: string;
  readonly displayName: string;
  readonly stableId: string;
  readonly featureType: string;
  readonly reference: ReferenceDescriptor;
  readonly strand: "unknown" | "forward" | "reverse";
}

export interface LocusSearchResult {
  readonly query: string;
  readonly normalizedQuery: string;
  readonly mode: "exact" | "prefix";
  readonly annotationName?: string;
  readonly annotationSha256?: string;
  readonly totalIndexedRecords: bigint;
  readonly hits: readonly LocusHit[];
  readonly truncated: boolean;
  readonly trace?: FeatureQueryTrace;
}

export interface SummaryQuery {
  readonly sample: string;
  readonly contig: string;
  readonly start: number;
  readonly end: number;
  readonly maxBins?: number;
  readonly signal?: AbortSignal;
  readonly trace?: boolean | ((trace: FeatureQueryTrace) => void);
}

export interface OverviewBin {
  readonly reference: ReferenceDescriptor;
  readonly level: number;
  readonly binSpan: number;
  readonly coveredBases: bigint;
  readonly tileCount: bigint;
  readonly encodedBytes: bigint;
  readonly decodedBytes: bigint;
  readonly nodeRecords: bigint;
  readonly edgeRecords: bigint;
  readonly gbwtRecords: bigint;
  readonly occurrences: bigint;
}

export interface SummaryResult {
  readonly query: Readonly<SummaryQuery>;
  readonly bins: readonly OverviewBin[];
  readonly trace?: FeatureQueryTrace;
}

export interface PangenomeArchive {
  readonly formatVersion: number;
  readonly semantics: HaplotypeSemantics;
  references(): readonly ReferenceDescriptor[];
  capabilities(): ArchiveCapabilities;
  info(options?: { signal?: AbortSignal }): Promise<ArchiveInfo>;
  searchLoci(query: LocusSearch): Promise<LocusSearchResult>;
  summary(query: SummaryQuery): Promise<SummaryResult>;
  query(query: RegionQuery): Promise<RegionResult>;
  /** Streams tiles as decoding completes; progressive event order is intentionally unspecified. */
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
