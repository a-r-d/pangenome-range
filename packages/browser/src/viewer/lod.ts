import type { OverviewBin } from "../reader/types.js";

export type ViewerDisplayMode = "overview" | "regional" | "detailed" | "base";

export interface ViewerComplexityBudgets {
  readonly compressedBytes: bigint;
  readonly decodedBytes: bigint;
  readonly nodeRecords: bigint;
  readonly edgeRecords: bigint;
  readonly occurrences: bigint;
}

export interface ViewerLodDecision {
  readonly mode: ViewerDisplayMode;
  readonly automaticDetail: boolean;
  readonly bpPerPixel: number;
  readonly estimates: ViewerComplexityBudgets;
  readonly budgets: ViewerComplexityBudgets;
  readonly limitingMetrics: readonly (keyof ViewerComplexityBudgets)[];
  readonly reason: string;
}

export const DEFAULT_COMPLEXITY_BUDGETS: ViewerComplexityBudgets = {
  compressedBytes: 4n * 1024n * 1024n,
  decodedBytes: 32n * 1024n * 1024n,
  nodeRecords: 50_000n,
  edgeRecords: 100_000n,
  occurrences: 2_000_000n,
};

/** Choose detail from measured summary counters as well as visual scale. */
export function chooseViewerLod(
  bins: readonly OverviewBin[],
  interval: { readonly start: number; readonly end: number },
  viewportPixels: number,
  budgets: ViewerComplexityBudgets = DEFAULT_COMPLEXITY_BUDGETS,
  forceDetail = false,
): ViewerLodDecision {
  if (
    !Number.isSafeInteger(interval.start) ||
    !Number.isSafeInteger(interval.end) ||
    interval.end <= interval.start
  ) {
    throw new RangeError("LOD interval must be a nonempty safe-integer range.");
  }
  if (!Number.isFinite(viewportPixels) || viewportPixels <= 0) {
    throw new RangeError("LOD viewport width must be positive and finite.");
  }
  const estimates = bins.reduce<ViewerComplexityBudgets>(
    (total, bin) => ({
      compressedBytes: total.compressedBytes + bin.encodedBytes,
      decodedBytes: total.decodedBytes + bin.decodedBytes,
      nodeRecords: total.nodeRecords + bin.nodeRecords,
      edgeRecords: total.edgeRecords + bin.edgeRecords,
      occurrences: total.occurrences + bin.occurrences,
    }),
    {
      compressedBytes: 0n,
      decodedBytes: 0n,
      nodeRecords: 0n,
      edgeRecords: 0n,
      occurrences: 0n,
    },
  );
  const limitingMetrics = (
    Object.keys(budgets) as (keyof ViewerComplexityBudgets)[]
  ).filter((key) => estimates[key] > budgets[key]);
  const bpPerPixel = (interval.end - interval.start) / viewportPixels;
  const fits = limitingMetrics.length === 0;
  const baseScale = bpPerPixel <= 8 && estimates.nodeRecords <= 8_000n;
  const detailedScale = bpPerPixel <= 400;
  const regionalScale = bpPerPixel <= 20_000;
  const mode: ViewerDisplayMode =
    forceDetail && fits
      ? baseScale
        ? "base"
        : "detailed"
      : fits && baseScale
        ? "base"
        : fits && detailedScale
          ? "detailed"
          : regionalScale
            ? "regional"
            : "overview";
  const automaticDetail = fits && (mode === "detailed" || mode === "base");
  const reason = automaticDetail
    ? `${mode} graph fits all five archive-derived complexity budgets.`
    : limitingMetrics.length > 0
      ? `Detailed graph held back by ${limitingMetrics.join(", ")} budget${limitingMetrics.length === 1 ? "" : "s"}.`
      : `Summary retained at ${formatBpPerPixel(bpPerPixel)} per horizontal pixel.`;
  return {
    mode,
    automaticDetail,
    bpPerPixel,
    estimates,
    budgets,
    limitingMetrics,
    reason,
  };
}

export function recommendedSummaryBins(viewportPixels: number): number {
  if (!Number.isFinite(viewportPixels) || viewportPixels <= 0) return 64;
  return Math.max(32, Math.min(1024, Math.ceil(viewportPixels / 4)));
}

function formatBpPerPixel(value: number): string {
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)} Mbp`;
  if (value >= 1_000) return `${(value / 1_000).toFixed(1)} kbp`;
  return `${value.toFixed(1)} bp`;
}
