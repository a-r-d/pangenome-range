<script setup lang="ts">
// biome-ignore-all lint/correctness/noUnusedVariables: Vue template bindings are consumed outside the script AST.
import type {
  ArchiveInfo,
  QueryTrace,
  RegionPlan,
} from "pangenome-range/reader";
import type { TubeMapModel } from "pangenome-range/viewer";
import type { BrowserMetrics, BrowserPhase } from "./types";

defineProps<{
  phase: BrowserPhase;
  message: string;
  info?: ArchiveInfo;
  plan?: RegionPlan;
  trace?: QueryTrace;
  model?: TubeMapModel;
  metrics: BrowserMetrics;
}>();

function bytes(value: bigint | number | undefined): string {
  if (value === undefined) return "—";
  const number = typeof value === "bigint" ? Number(value) : value;
  if (number >= 1024 ** 3) return `${(number / 1024 ** 3).toFixed(1)} GiB`;
  if (number >= 1024 ** 2) return `${(number / 1024 ** 2).toFixed(1)} MiB`;
  if (number >= 1024) return `${(number / 1024).toFixed(1)} KiB`;
  return `${number} B`;
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
    <span>{{ bytes(trace?.totalBytes ?? plan?.compressedBytes) }} fetched</span>
    <span>{{ trace?.requestRanges.length ?? plan?.ranges.length ?? 0 }} ranges</span>
    <span>{{ metrics.completeMs === undefined ? '—' : `${metrics.completeMs.toFixed(0)} ms` }}</span>
    <span>{{ bytes(info?.archiveBytes) }} archive</span>
    <span>{{ trace ? 'SHA-256 verified' : 'integrity pending' }}</span>
  </footer>
</template>
