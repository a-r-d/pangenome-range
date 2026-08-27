<script setup lang="ts">
// biome-ignore-all lint/correctness/noUnusedVariables: Vue template bindings are consumed outside the script AST.
import type { ViewerLayerState } from "pangenome-range/viewer";

const props = defineProps<{
  mode: "hidden" | "preview" | "detail";
  locus: string;
  coordinate: string;
  layers: ViewerLayerState;
}>();

const emit = defineEmits<{
  toggleTopology: [];
  toggleTraversals: [];
  toggleSequence: [];
}>();
</script>

<template>
  <section class="graph-surface" :data-mode="props.mode" aria-label="Detailed graph viewport">
    <header class="graph-surface-toolbar">
      <div>
        <span>{{ props.mode === 'preview' ? 'Live detailed preview' : 'Detailed graph' }}</span>
        <h1 v-if="props.mode === 'detail'">{{ props.locus }}</h1>
        <strong v-else>{{ props.locus }}</strong>
        <small>{{ props.coordinate }}</small>
      </div>
      <div class="segmented graph-tabs" aria-label="Detailed graph layers">
        <button type="button" :aria-pressed="props.layers.topology" @click="emit('toggleTopology')">Graph</button>
        <button type="button" :aria-pressed="props.layers.traversals" @click="emit('toggleTraversals')">Paths</button>
        <button type="button" :aria-pressed="props.layers.sequenceLabels" @click="emit('toggleSequence')">Sequence</button>
      </div>
    </header>
    <div class="graph-legend" aria-hidden="true"><span class="reference">Reference backbone</span><span class="alternate">Alternate branches</span><span class="traversal">Local traversal evidence</span></div>
    <div class="graph-viewer-slot"><slot></slot></div>
  </section>
</template>
