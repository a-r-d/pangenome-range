<script setup lang="ts">
// biome-ignore-all lint/correctness/noUnusedVariables: Vue template bindings are consumed outside the script AST.
import type { LocusHit } from "pangenome-range/reader";
import HelpTooltip from "./HelpTooltip.vue";
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
  namedPathHintAvailable: boolean;
  namedPathHintVisible: boolean;
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
  verticalOut: [];
  verticalIn: [];
  archive: [];
  share: [];
  showNamedPathHint: [];
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
    <a class="browser-brand" href="../" aria-label="pangenome-range documentation" title="Open the pangenome-range documentation">P</a>
    <button class="icon-button" type="button" aria-label="Back" title="Return to the previous genomic view" @click="emit('back')">←</button>
    <button class="icon-button" type="button" aria-label="Forward" title="Move to the next genomic view" @click="emit('forward')">→</button>
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
      <button type="button" aria-label="Zoom out" title="Zoom out horizontally around the graph center" @click="emit('zoomOut')">−</button>
      <button type="button" aria-label="Zoom in" title="Zoom in horizontally around the graph center" @click="emit('zoomIn')">+</button>
      <button type="button" title="Fit the current graph horizontally and vertically" @click="emit('fit')">Fit</button>
    </div>
    <div class="toolbar-vertical" role="group" aria-label="Vertical spacing">
      <span aria-hidden="true" title="Vertical lane spacing">↕</span>
      <button type="button" aria-label="Decrease vertical spacing" title="Compress alternate graph lanes vertically" @click="emit('verticalOut')">−</button>
      <button type="button" aria-label="Increase vertical spacing" title="Expand alternate graph lanes vertically" @click="emit('verticalIn')">+</button>
    </div>
    <label class="toolbar-select" title="Choose how many highest-weight tile-local traversal patterns to draw. These are not named samples.">
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
      <summary title="Change graph simplification, base display, and tile-boundary settings">Options</summary>
      <div class="toolbar-menu__panel">
        <div class="toolbar-option-row">
          <label><input :checked="options.simplifyLinearChains" type="checkbox" @change="updateOption(options, 'simplifyLinearChains', ($event.target as HTMLInputElement).checked)" /> Simplify linear chains</label>
          <HelpTooltip text="Collapse non-branching node runs into display groups. Graph branches and data are preserved, and a selected group can be expanded." />
        </div>
        <div v-if="namedPathHintAvailable" class="toolbar-option-row">
          <button type="button" @click="emit('showNamedPathHint')">{{ namedPathHintVisible ? 'Named-path hint is visible' : 'Show named-path hint' }}</button>
          <HelpTooltip text="The hint points to colored tile-local traversals whose exact source-path membership can be inspected." />
        </div>
        <div class="toolbar-option-row">
          <label>Show bases <select :value="options.showBases" aria-label="Base display mode" @change="updateOption(options, 'showBases', ($event.target as HTMLSelectElement).value as GraphOptions['showBases'])"><option value="automatic">Automatic</option><option value="on">On</option><option value="off">Off</option></select></label>
          <HelpTooltip text="Automatic shows short sequences when they fit. On shows eligible sequences up to 48 bases. Off hides sequence letters." />
        </div>
        <div class="toolbar-option-row">
          <label><input :checked="options.showTileBoundaries" type="checkbox" @change="updateOption(options, 'showTileBoundaries', ($event.target as HTMLInputElement).checked)" /> Source-tile boundaries</label>
          <HelpTooltip text="Show dashed T1, T2, and other markers where independently fetched archive tiles begin. Tile-local patterns do not continue across these boundaries." />
        </div>
      </div>
    </details>
    <button class="toolbar-button" type="button" title="Choose the configured archive, a remote .pngr URL, or a local .pngr file" @click="emit('archive')">Source</button>
    <button class="toolbar-button" type="button" title="Create a link to this exact genomic region" @click="emit('share')">Share</button>
  </header>
</template>
