<script setup lang="ts">
// biome-ignore-all lint/correctness/noUnusedVariables: Vue template bindings are consumed outside the script AST.
import type { LocalPattern } from "pangenome-range/viewer";

defineProps<{ pattern: LocalPattern }>();
const emit = defineEmits<{ close: [] }>();
</script>

<template>
  <aside class="browser-inspector" aria-label="Pattern inspector">
    <header><span>Local traversal pattern</span><button type="button" aria-label="Close inspector" @click="emit('close')">×</button></header>
    <h2>{{ pattern.id }} × {{ pattern.weight.toLocaleString() }}</h2>
    <p class="inspector-warning">Anonymous, distinct, integer-weighted evidence local to {{ pattern.tileKey }}. This is not a named person, allele frequency, or path that can be stitched across tiles.</p>
    <dl>
      <div><dt>Source tile</dt><dd>{{ pattern.tileKey }}</dd></div>
      <div><dt>Core interval</dt><dd>{{ pattern.tileStart.toLocaleString() }}–{{ pattern.tileEnd.toLocaleString() }}</dd></div>
      <div><dt>Node visits</dt><dd>{{ pattern.orientedNodes.length.toLocaleString() }}</dd></div>
      <div><dt>Integer weight</dt><dd>{{ pattern.weight.toLocaleString() }}</dd></div>
    </dl>
  </aside>
</template>
