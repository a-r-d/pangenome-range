import type { ChunkDecompressor } from "pangenome-range/reader";
import type {
  DecompressionMeasurement,
  DistributionSummary,
  SerializableRequest,
} from "./types.js";

export function percentile(
  values: readonly number[],
  fraction: number,
): number | null {
  if (values.length === 0) return null;
  const sorted = [...values].sort((left, right) => left - right);
  const index = Math.min(
    sorted.length - 1,
    Math.max(0, Math.ceil(fraction * sorted.length) - 1),
  );
  return sorted[index] as number;
}

export function distribution(values: readonly number[]): DistributionSummary {
  return {
    count: values.length,
    p50: percentile(values, 0.5),
    p95: percentile(values, 0.95),
    max: values.length === 0 ? null : Math.max(...values),
  };
}

export function decompressionSummary(
  samplesMs: readonly number[],
): DecompressionMeasurement {
  return {
    calls: samplesMs.length,
    samplesMs: [...samplesMs],
    totalMs: samplesMs.reduce((total, sample) => total + sample, 0),
    p50Ms: percentile(samplesMs, 0.5),
    p95Ms: percentile(samplesMs, 0.95),
  };
}

export class TimedDecompressor implements ChunkDecompressor {
  readonly #delegate: ChunkDecompressor;
  readonly #samplesMs: number[] = [];

  constructor(delegate: ChunkDecompressor) {
    this.#delegate = delegate;
  }

  get samplesMs(): readonly number[] {
    return [...this.#samplesMs];
  }

  clear(): void {
    this.#samplesMs.length = 0;
  }

  async decompress(
    compressed: Uint8Array,
    expectedLength: number,
    options?: { signal?: AbortSignal },
  ): Promise<Uint8Array> {
    const started = performance.now();
    try {
      return await this.#delegate.decompress(
        compressed,
        expectedLength,
        options,
      );
    } finally {
      this.#samplesMs.push(performance.now() - started);
    }
  }
}

function requestRange(
  request: SerializableRequest,
): { start: bigint; end: bigint } | undefined {
  if (request.offset !== undefined && request.length !== undefined) {
    const start = BigInt(request.offset);
    return { start, end: start + BigInt(request.length) };
  }
  const match = /^bytes=(\d+)-(\d+)$/.exec(request.range ?? "");
  if (match === null) return undefined;
  return {
    start: BigInt(match[1] as string),
    end: BigInt(match[2] as string) + 1n,
  };
}

export function requestBytes(requests: readonly SerializableRequest[]): {
  total: number;
  unique: number;
  duplicate: number;
} {
  const ranges = requests
    .map(requestRange)
    .filter(
      (value): value is { start: bigint; end: bigint } => value !== undefined,
    )
    .sort((left, right) =>
      left.start < right.start ? -1 : left.start > right.start ? 1 : 0,
    );
  let unique = 0n;
  let current: { start: bigint; end: bigint } | undefined;
  for (const range of ranges) {
    if (current === undefined) {
      current = { ...range };
    } else if (range.start <= current.end) {
      if (range.end > current.end) current.end = range.end;
    } else {
      unique += current.end - current.start;
      current = { ...range };
    }
  }
  if (current !== undefined) unique += current.end - current.start;
  const total = requests.reduce((sum, request) => sum + request.bytes, 0);
  const uniqueNumber = Number(unique);
  if (!Number.isSafeInteger(uniqueNumber)) {
    throw new RangeError(
      "request unique-byte count exceeds safe integer range",
    );
  }
  return {
    total,
    unique: uniqueNumber,
    duplicate: Math.max(0, total - uniqueNumber),
  };
}

export function observedRequestRounds(
  requests: readonly SerializableRequest[],
): number | undefined {
  if (requests.length === 0) return 0;
  if (requests.some((request) => request.startedAtMs === undefined)) {
    return undefined;
  }
  const ordered = [...requests].sort(
    (left, right) =>
      (left.startedAtMs as number) - (right.startedAtMs as number),
  );
  let rounds = 0;
  let currentRoundEnd = Number.NEGATIVE_INFINITY;
  for (const request of ordered) {
    const started = request.startedAtMs as number;
    if (started >= currentRoundEnd) {
      rounds += 1;
      currentRoundEnd = started + request.elapsedMs;
    } else {
      currentRoundEnd = Math.max(currentRoundEnd, started + request.elapsedMs);
    }
  }
  return rounds;
}
