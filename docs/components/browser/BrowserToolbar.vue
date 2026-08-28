<script setup lang="ts">
// biome-ignore-all lint/correctness/noUnusedVariables: Vue template bindings are consumed outside the script AST.
import type { LocusHit } from "pangenome-range/reader";
import LocationSearch from "./LocationSearch.vue";
import type { GraphOptions } from "./types";

defineProps<{
  command: string;
  suggestions: readonly LocusHit[];
  activeSuggestion: number;
  searching: boolean;
  disabled: boolean;
  searchMessage?: string;
  options: GraphOptions;
}>();
const emit = defineEmits<{
  "update:command": [value: string];
  "update:activeSuggestion": [index: number];
  "update:options": [options: GraphOptions];
  submit: [];
  select: [hit: LocusHit];
  closeSearch: [];
  back: [];
  forward: [];
  zoomOut: [];
  zoomIn: [];
  fit: [];
  archive: [];
  share: [];
}>();

function updateOption<Key extends keyof GraphOptions>(
  options: GraphOptions,
  key: Key,
  value: GraphOptions[Key],
): void {
  emit("update:options", { ...options, [key]: value });
}
</script>

<template>
  <header class="browser-toolbar">
    <a class="browser-brand" href="../" aria-label="pangenome-range documentation">P</a>
    <button class="icon-button" type="button" aria-label="Back" @click="emit('back')">←</button>
    <button class="icon-button" type="button" aria-label="Forward" @click="emit('forward')">→</button>
    <LocationSearch
      :model-value="command"
      :suggestions="suggestions"
      :active-index="activeSuggestion"
      :busy="searching"
      :disabled="disabled"
      :message="searchMessage"
      @update:model-value="emit('update:command', $event)"
      @update:active-index="emit('update:activeSuggestion', $event)"
      @submit="emit('submit')"
      @select="emit('select', $event)"
      @close="emit('closeSearch')"
    />
    <div class="toolbar-zoom" role="group" aria-label="Graph zoom">
      <button type="button" aria-label="Zoom out" @click="emit('zoomOut')">−</button>
      <button type="button" aria-label="Zoom in" @click="emit('zoomIn')">+</button>
      <button type="button" @click="emit('fit')">Fit</button>
    </div>
    <label class="toolbar-select">
      <span class="visually-hidden">Local patterns</span>
      <select
        :value="options.patternCount"
        aria-label="Local-pattern count"
        @change="updateOption(options, 'patternCount', Number(($event.target as HTMLSelectElement).value) as 0 | 4 | 8 | 16)"
      >
        <option :value="0">Patterns 0</option>
        <option :value="4">Patterns 4</option>
        <option :value="8">Patterns 8</option>
        <option :value="16">Patterns 16</option>
      </select>
    </label>
    <details class="toolbar-menu">
      <summary>Options</summary>
      <div class="toolbar-menu__panel">
        <label><input :checked="options.simplifyLinearChains" type="checkbox" @change="updateOption(options, 'simplifyLinearChains', ($event.target as HTMLInputElement).checked)" /> Simplify linear chains</label>
        <label>Show bases <select :value="options.showBases" @change="updateOption(options, 'showBases', ($event.target as HTMLSelectElement).value as GraphOptions['showBases'])"><option value="automatic">Automatic</option><option value="on">On</option><option value="off">Off</option></select></label>
        <label><input :checked="options.showTileBoundaries" type="checkbox" @change="updateOption(options, 'showTileBoundaries', ($event.target as HTMLInputElement).checked)" /> Source-tile boundaries</label>
      </div>
    </details>
    <button class="toolbar-button" type="button" @click="emit('archive')">Archive</button>
    <button class="toolbar-button" type="button" @click="emit('share')">Share</button>
  </header>
</template>
