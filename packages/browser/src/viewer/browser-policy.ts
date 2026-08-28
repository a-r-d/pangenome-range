import type { LocusHit, RegionPlan, RegionQuery } from "../reader/types.js";

export interface GraphRegionLimits {
  readonly maxSpanBp: number;
  readonly maxCompressedBytes: bigint;
  readonly maxPayloads: number;
}

export interface GraphRegionDecision {
  readonly allowed: boolean;
  readonly exceeded: readonly ("span" | "compressedBytes" | "payloads")[];
}

export const DEFAULT_GRAPH_REGION_LIMITS: GraphRegionLimits = {
  maxSpanBp: 100_000,
  maxCompressedBytes: 4n * 1024n * 1024n,
  maxPayloads: 8,
};

export function decideGraphRegion(
  query: Readonly<RegionQuery>,
  plan: Readonly<RegionPlan>,
  limits: GraphRegionLimits = DEFAULT_GRAPH_REGION_LIMITS,
): GraphRegionDecision {
  const exceeded: GraphRegionDecision["exceeded"][number][] = [];
  if (query.end - query.start > limits.maxSpanBp) exceeded.push("span");
  if (plan.compressedBytes > limits.maxCompressedBytes) {
    exceeded.push("compressedBytes");
  }
  if (plan.selectedChunks > limits.maxPayloads) exceeded.push("payloads");
  return { allowed: exceeded.length === 0, exceeded };
}

export function recommendedGraphRegion(
  query: Readonly<RegionQuery>,
  reference: { readonly start: number; readonly end: number },
  selectedLocus?: Pick<LocusHit, "reference">,
  span = 40_000,
): RegionQuery {
  if (!Number.isSafeInteger(span) || span <= 0) {
    throw new RangeError(
      "recommended graph span must be a positive safe integer",
    );
  }
  const center =
    selectedLocus === undefined
      ? Math.floor((query.start + query.end) / 2)
      : Math.floor(
          (selectedLocus.reference.start + selectedLocus.reference.end) / 2,
        );
  const available = Math.max(1, reference.end - reference.start);
  const boundedSpan = Math.min(span, available);
  let start = Math.max(reference.start, center - Math.floor(boundedSpan / 2));
  let end = start + boundedSpan;
  if (end > reference.end) {
    end = reference.end;
    start = Math.max(reference.start, end - boundedSpan);
  }
  return {
    sample: query.sample,
    contig: query.contig,
    start,
    end,
    ...(query.context === undefined ? {} : { context: query.context }),
  };
}
