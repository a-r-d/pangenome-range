import type { QueryTrace } from "pangenome-range/reader";

export const WORKLOAD_SCHEMA_VERSION = 1 as const;
export const RESULT_SCHEMA_VERSION = 1 as const;

export interface BenchmarkQuery {
  readonly id: string;
  readonly class: string;
  readonly sample: string;
  readonly contig: string;
  readonly start: number;
  readonly end: number;
  readonly context: number;
  readonly expectedCanonicalHash?: string;
  readonly expectedError?: string;
}

export interface BenchmarkWorkload {
  readonly schemaVersion: typeof WORKLOAD_SCHEMA_VERSION;
  readonly archiveSha256: string;
  readonly seed?: string;
  readonly queries: readonly BenchmarkQuery[];
}

export type DecoderName = "pure-js" | "wasm";
export type CacheMode = "cold" | "warm";

export interface ArchiveIdentity {
  readonly location: string;
  readonly kind: "file" | "http";
  readonly size: string;
  readonly sha256?: string;
  readonly etag?: string;
}

export interface SerializableRequest {
  readonly sequence: number;
  readonly queryId: string;
  readonly scenario: string;
  readonly browser?: string;
  readonly decoder: DecoderName;
  readonly method: string;
  readonly range: string | null;
  readonly offset?: string;
  readonly length?: number;
  readonly status?: number;
  readonly bytes: number;
  readonly elapsedMs: number;
  readonly startedAt?: string;
  readonly startedAtMs?: number;
  readonly connectionId?: string;
  readonly source: "reader" | "origin";
}

export interface DecompressionMeasurement {
  readonly calls: number;
  readonly samplesMs: readonly number[];
  readonly totalMs: number;
  readonly p50Ms: number | null;
  readonly p95Ms: number | null;
}

export interface QueryMeasurement {
  readonly queryId: string;
  readonly queryClass: string;
  readonly sample: string;
  readonly contig: string;
  readonly start: number;
  readonly end: number;
  readonly context: number;
  readonly scenario: string;
  readonly cacheMode: CacheMode;
  readonly httpCachePolicy: string;
  readonly decoder: DecoderName;
  readonly browser?: string;
  readonly totalMs: number;
  readonly openMs: number;
  readonly queryMs: number;
  readonly actualRequests: number;
  readonly actualBytes: number;
  readonly actualUniqueBytes: number;
  readonly actualDuplicateBytes: number;
  readonly actualRequestRounds?: number;
  readonly readerObservedRequests?: number;
  readonly readerObservedBytes?: number;
  readonly sourceOriginReconciled?: boolean;
  readonly plannedRequests: number;
  readonly plannedBytes: number;
  readonly dependencyRounds: number;
  readonly cacheHits: QueryTrace["cacheHits"];
  readonly decompression: DecompressionMeasurement;
  readonly integrityMs: number;
  /** Interval-union decompression wall time from the reader trace. */
  readonly decompressionWallMs: number;
  /** Aggregate decompression task time; may exceed wall time. */
  readonly decompressionTaskMs: number;
  readonly decodeMs: number;
  readonly mergeMs: number;
  readonly selectedChunks: number;
  readonly selectedNodes: number;
  readonly selectedTraversals: number;
  readonly canonicalHash?: string;
  readonly expectedCanonicalHash?: string;
  readonly correctness: boolean;
  readonly error?: string;
  readonly expectedError?: string;
  readonly peakHeapBytes?: number;
  readonly performanceMarks?: Readonly<Record<string, number>>;
}

export interface DecoderSummary {
  readonly name: DecoderName;
  readonly initializationMs: number;
  readonly javascriptBytes: number;
  readonly wasmBytes: number;
  readonly decompression: DecompressionMeasurement;
  readonly peakHeapBytes: number | null;
  readonly limitation?: string;
}

export interface DistributionSummary {
  readonly count: number;
  readonly p50: number | null;
  readonly p95: number | null;
  readonly max: number | null;
}

export interface BenchmarkSummary {
  readonly schemaVersion: typeof RESULT_SCHEMA_VERSION;
  readonly runId: string;
  readonly kind: "node" | "browser";
  readonly archive: ArchiveIdentity;
  readonly workloadSha256: string;
  readonly measurements: readonly QueryMeasurement[];
  readonly decoderSummaries: readonly DecoderSummary[];
  readonly totals: {
    readonly queries: number;
    readonly passed: number;
    readonly failed: number;
    readonly requests: number;
    readonly responseBytes: number;
    readonly uniqueBytes: number;
    readonly duplicateBytes: number;
  };
  readonly latencyMs: DistributionSummary;
  readonly limitations: readonly string[];
}
