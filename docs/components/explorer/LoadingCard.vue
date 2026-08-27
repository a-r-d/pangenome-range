<script setup lang="ts">
// biome-ignore-all lint/correctness/noUnusedVariables: Vue template bindings are consumed outside the script AST.
import type { ExplorerPhase } from "./types";

const props = defineProps<{
  visible: boolean;
  phase: ExplorerPhase;
  locus: string;
  openTime: string;
  summaryBytes: string;
  expectedTransfer: string;
  remoteRanges: number;
  completedTiles: number;
  plannedTiles: number;
  reason: string;
}>();

const emit = defineEmits<{ cancel: [] }>();

const complete = (stage: "archive" | "summary" | "plan" | "tiles") => {
  const order = {
    opening: 0,
    summary: 1,
    graph: 3,
    ready: 4,
    idle: 0,
    error: 0,
  }[props.phase];
  return order > { archive: 0, summary: 1, plan: 2, tiles: 3 }[stage];
};
</script>

<template>
  <div v-if="props.visible" class="loading-layer" role="status" aria-live="polite">
    <div class="loading-card">
      <div class="loading-heading"><span class="loading-orbit" aria-hidden="true"><i></i></span><div><p class="eyebrow">Range-addressed workflow</p><h2>{{ props.phase === 'opening' ? 'Opening archive' : `Loading ${props.locus}` }}</h2><p>Previous context stays visible while bounded range requests stream in.</p></div></div>
      <ol>
        <li :class="{ done: complete('archive'), active: props.phase === 'opening' }"><span></span><strong>Archive open</strong><em>{{ props.openTime }}</em></li>
        <li :class="{ done: complete('summary'), active: props.phase === 'summary' }"><span></span><strong>Summary ready</strong><em>{{ props.summaryBytes }}</em></li>
        <li :class="{ done: complete('plan'), active: props.phase === 'summary' }"><span></span><strong>Region planned</strong><em>{{ props.remoteRanges }} range{{ props.remoteRanges === 1 ? '' : 's' }}</em></li>
        <li :class="{ done: complete('tiles'), active: props.phase === 'graph' }"><span></span><strong>Graph tiles streaming</strong><em>{{ props.completedTiles }} / {{ props.plannedTiles || '—' }}</em></li>
        <li :class="{ done: props.phase === 'ready' }"><span></span><strong>Integrity, layout, and paint</strong><em>{{ props.phase === 'ready' ? 'verified' : '—' }}</em></li>
      </ol>
      <div class="loading-progress"><i :style="{ width: `${props.phase === 'opening' ? 12 : props.phase === 'summary' ? 38 : props.phase === 'graph' && props.plannedTiles ? 48 + (props.completedTiles / props.plannedTiles) * 42 : 100}%` }"></i></div>
      <div class="loading-facts"><article><span>Expected transfer</span><strong>{{ props.expectedTransfer }}</strong></article><article><span>Remote ranges</span><strong>{{ props.remoteRanges || '—' }}</strong></article><article><span>Decoder</span><strong>Pure JS zstd</strong></article><article><span>Why detail?</span><strong>{{ props.reason }}</strong></article></div>
      <div class="loading-meta"><span>Only selected byte ranges are transferred.</span><button type="button" @click="emit('cancel')">Cancel</button></div>
    </div>
  </div>
</template>
