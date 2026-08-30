<script setup lang="ts">
// biome-ignore-all lint/correctness/noUnusedVariables: Vue template bindings are consumed outside the script AST.
import type {
  ArchiveInfo,
  QueryTrace,
  RegionPlan,
} from "pangenome-range/reader";
import type { TubeMapModel } from "pangenome-range/viewer";
import type { BrowserMetrics, BrowserPhase, PatternEvidence } from "./types";

defineProps<{
  phase: BrowserPhase;
  message: string;
  info?: ArchiveInfo;
  plan?: RegionPlan;
  trace?: QueryTrace;
  model?: TubeMapModel;
  metrics: BrowserMetrics;
  identityEvidence?: PatternEvidence;
  pathMembershipAvailable: boolean;
}>();

function bytes(value: bigint | number | undefined): string {
  if (value === undefined) return "—";
  const number = typeof value === "bigint" ? Number(value) : value;
  if (number >= 1024 ** 3) return `${(number / 1024 ** 3).toFixed(1)} GiB`;
  if (number >= 1024 ** 2) return `${(number / 1024 ** 2).toFixed(1)} MiB`;
  if (number >= 1024) return `${(number / 1024).toFixed(1)} KiB`;
  return `${number} B`;
}

function fetchedBytes(
  trace: QueryTrace | undefined,
  plan: RegionPlan | undefined,
  evidence: PatternEvidence | undefined,
): bigint | number | undefined {
  const graph = trace?.totalBytes ?? plan?.compressedBytes;
  if (graph === undefined) return undefined;
  return (
    BigInt(graph) +
    BigInt(evidence?.membership.totalBytes ?? 0) +
    BigInt(evidence?.catalog.totalBytes ?? 0)
  );
}

function fetchedRanges(
  trace: QueryTrace | undefined,
  plan: RegionPlan | undefined,
  evidence: PatternEvidence | undefined,
): number {
  return (
    (trace?.requestRanges.length ?? plan?.ranges.length ?? 0) +
    (evidence?.membership.requestRanges.length ?? 0) +
    (evidence?.catalog.requestRanges.length ?? 0)
  );
}
</script>

<template>
  <footer
    class="browser-status"
    role="status"
    :data-phase="phase"
    :data-open-ms="metrics.openMs"
    :data-first-tile-ms="metrics.firstTileMs"
    :data-complete-ms="metrics.completeMs"
    :data-layout-ms="metrics.layoutMs"
    :data-svg-elements="metrics.svgElements"
  >
    <span class="status-dot"></span>
    <strong>{{ message }}</strong>
    <span>{{ model?.counts.tiles ?? 0 }}/{{ plan?.selectedChunks ?? 0 }} tiles</span>
    <span>{{ bytes(fetchedBytes(trace, plan, identityEvidence)) }} read</span>
    <span>{{ fetchedRanges(trace, plan, identityEvidence) }} ranges</span>
    <span>{{ metrics.completeMs === undefined ? '—' : `${metrics.completeMs.toFixed(0)} ms` }}</span>
    <span>{{ info ? `${(Number(info.archiveBytes) / 1e9).toFixed(2)} GB remote` : '— remote' }}</span>
    <span>{{ pathMembershipAvailable ? 'named paths available' : 'graph-only identity' }}</span>
    <span>{{ trace ? 'verified' : 'integrity pending' }}</span>
  </footer>
</template>
