<script setup lang="ts">
// biome-ignore-all lint/correctness/noUnusedVariables: Vue template bindings are consumed outside the script AST.
import type { LocusHit } from "pangenome-range/reader";
import type { SearchState } from "./types";

const props = defineProps<{
  open: boolean;
  suggestions: readonly LocusHit[];
  activeSuggestion: number;
  recentSearches: readonly string[];
  searchState: SearchState;
  searchMessage: string;
}>();

const emit = defineEmits<{
  close: [];
  activate: [index: number];
  select: [hit: LocusHit, intent: "detail" | "overview"];
  recent: [value: string];
}>();

function formatCoordinate(value: number): string {
  return new Intl.NumberFormat("en-US", { maximumFractionDigits: 0 }).format(
    value,
  );
}

function affordance(hit: LocusHit): "detail" | "overview" {
  return hit.reference.end - hit.reference.start <= 120_000
    ? "detail"
    : "overview";
}
</script>

<template>
  <div v-if="props.open" class="palette-backdrop" @click="emit('close')"></div>
  <section v-if="props.open" class="suggestions" role="listbox" aria-label="Locus suggestions">
    <header><div><span>Archive-native search</span><strong>Genes, aliases, stable IDs, or coordinates</strong></div><button type="button" @click="emit('close')">Esc</button></header>
    <div v-if="props.searchState === 'searching'" class="suggestion-state"><span class="search-spinner"></span>Searching compressed archive index…</div>
    <button
      v-for="(hit, index) in props.suggestions"
      :id="`locus-option-${index}`"
      :key="`${hit.stableId}:${hit.reference.sample}:${hit.reference.start}:${hit.matchedName}`"
      type="button"
      role="option"
      :aria-selected="index === props.activeSuggestion"
      @mouseenter="emit('activate', index)"
      @click="emit('select', hit, affordance(hit))"
    >
      <span class="result-identity"><strong>{{ hit.displayName }}</strong><em>{{ hit.matchedName !== hit.displayName ? `alias match · ${hit.matchedName}` : hit.featureType }}</em></span>
      <span class="result-provenance"><b>{{ hit.featureType }} · {{ hit.stableId }}</b><em>{{ hit.reference.sample }} · {{ hit.reference.contig }}:{{ formatCoordinate(hit.reference.start) }}–{{ formatCoordinate(hit.reference.end) }}</em></span>
      <span class="result-affordance" :data-intent="affordance(hit)">{{ affordance(hit) === 'detail' ? 'Load detailed graph' : 'Overview first' }} <b>→</b></span>
    </button>
    <div v-if="props.suggestions.length === 0 && props.recentSearches.length > 0" class="recent-searches"><h3>Recent</h3><div><button v-for="recent in props.recentSearches" :key="recent" type="button" @click="emit('recent', recent)"><strong>{{ recent }}</strong></button></div></div>
    <div v-if="props.searchState === 'index-absent'" class="suggestion-state">This archive has no named-locus index. Coordinate navigation remains available.</div>
    <div v-else-if="props.searchState === 'index-empty'" class="suggestion-state">The named-locus index is present but empty.</div>
    <div v-else-if="props.searchState === 'truncated'" class="suggestion-state">First {{ props.suggestions.length }} results shown · archive result limit reached</div>
    <div v-else-if="props.searchState === 'no-matches'" class="suggestion-state">No matching archive locus</div>
    <div v-else-if="props.searchState === 'failed'" class="suggestion-state error-text">{{ props.searchMessage }}</div>
    <footer><span><kbd>↑</kbd><kbd>↓</kbd> Navigate</span><span><kbd>↵</kbd> Open</span><span>Exact and alias matches come from the archive itself</span></footer>
  </section>
</template>
