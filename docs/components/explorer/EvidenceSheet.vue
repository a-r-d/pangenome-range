<script setup lang="ts">
// biome-ignore-all lint/correctness/noUnusedVariables: Vue template bindings are consumed outside the script AST.
import type { FeatureQueryTrace, QueryTrace } from "pangenome-range/reader";
import type { ViewerPerformanceSnapshot } from "pangenome-range/viewer";
import type { ExplorerPhase } from "./types";

const props = defineProps<{
  open: boolean;
  phase: ExplorerPhase;
  label: string;
  summaryTrace?: FeatureQueryTrace;
  queryTrace?: QueryTrace;
  bytes: number | bigint | undefined;
  tiles: number;
  wallMs?: number;
  openMs?: number;
  summaryPaintMs?: number;
  performance?: ViewerPerformanceSnapshot;
  technicalMode: boolean;
}>();

const emit = defineEmits<{
  toggle: [];
  "update:technicalMode": [value: boolean];
}>();

function formatBytes(value: number | bigint | undefined): string {
  if (value === undefined) return "—";
  const bytes = typeof value === "bigint" ? Number(value) : value;
  if (bytes < 1_024) return `${bytes} B`;
  if (bytes < 1_048_576) return `${(bytes / 1_024).toFixed(1)} KiB`;
  if (bytes < 1_073_741_824) return `${(bytes / 1_048_576).toFixed(1)} MiB`;
  return `${(bytes / 1_073_741_824).toFixed(2)} GiB`;
}
const formatMs = (value?: number) =>
  value === undefined ? "—" : `${value.toFixed(1)} ms`;
</script>

<template>
  <section class="evidence" :class="{ open: props.open }" aria-label="Range and performance evidence">
    <button type="button" class="evidence-toggle" :aria-expanded="props.open" @click="emit('toggle')"><span class="source-dot" :data-state="props.phase"></span><strong>{{ props.label }}</strong><span>{{ formatBytes(props.bytes) }}{{ props.queryTrace === undefined ? ' planned' : ' new' }} · {{ props.tiles }} tiles {{ props.queryTrace === undefined ? 'planned' : 'ready' }} · {{ formatMs(props.wallMs) }}</span><svg viewBox="0 0 24 24" aria-hidden="true"><path :d="props.open ? 'm6 15 6-6 6 6' : 'm6 9 6 6 6-6'"></path></svg></button>
    <div v-if="props.open" class="evidence-body"><div class="timing-strip"><article><span>Object open</span><strong>{{ formatMs(props.openMs) }}</strong></article><article><span>First summary paint</span><strong>{{ formatMs(props.summaryPaintMs) }}</strong></article><article><span>First graph tile</span><strong>{{ formatMs(props.performance?.firstTilePaintMs) }}</strong></article><article><span>Query complete</span><strong>{{ formatMs(props.wallMs) }}</strong></article><article><span>Layout</span><strong>{{ formatMs(props.performance?.layoutMs) }}</strong></article><article><span>Paint</span><strong>{{ formatMs(props.performance?.paintMs) }}</strong></article></div><div class="waterfall"><div v-for="(range, index) in [...(props.summaryTrace?.requestRanges ?? []), ...(props.queryTrace?.requestRanges ?? [])]" :key="`${range.offset}:${range.length}:${index}`"><span>{{ range.layer }}</span><i :style="{ width: `${Math.max(2, Math.min(100, (range.length / Math.max(1, props.queryTrace?.totalBytes ?? props.summaryTrace?.totalBytes ?? range.length)) * 100))}%` }"></i><code>{{ range.offset.toString() }} + {{ formatBytes(range.length) }}</code></div><p v-if="props.summaryTrace === undefined && props.queryTrace === undefined">No traced request has completed yet.</p></div><label class="check"><input :checked="props.technicalMode" type="checkbox" @change="emit('update:technicalMode', ($event.currentTarget as HTMLInputElement).checked)" />Technical evidence mode</label><p v-if="props.technicalMode" class="technical-note">Canonical hash {{ props.queryTrace?.canonicalHash ?? '—' }}. Integrity {{ formatMs(props.queryTrace?.integrityMs) }}, decompression wall {{ formatMs(props.queryTrace?.decompressionMs) }}, regional decode {{ formatMs(props.queryTrace?.decodeMs) }}, graph merge {{ formatMs(props.queryTrace?.mergeMs) }}.</p></div>
  </section>
</template>
