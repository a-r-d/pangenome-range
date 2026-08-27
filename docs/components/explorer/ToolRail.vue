<script setup lang="ts">
// biome-ignore-all lint/correctness/noUnusedVariables: Vue template bindings are consumed outside the script AST.
import type { ExplorerScreen } from "./types";

type ToolPanel = "navigate" | "layers" | "tracks" | "detail" | null;

const props = defineProps<{
  screen: ExplorerScreen;
  activePanel: ToolPanel;
}>();

const emit = defineEmits<{
  home: [];
  panel: [panel: Exclude<ToolPanel, null>];
  source: [];
  help: [];
}>();
</script>

<template>
  <nav class="tool-rail" aria-label="Explorer tools">
    <button type="button" :aria-pressed="props.screen === 'showcase'" title="Showcase" @click="emit('home')"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 11 12 4l8 7v8a1 1 0 0 1-1 1h-5v-6h-4v6H5a1 1 0 0 1-1-1v-8Z"></path></svg><span>Showcase</span></button>
    <button type="button" :aria-pressed="props.activePanel === 'navigate'" title="Navigate" @click="emit('panel', 'navigate')"><svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="9"></circle><path d="m15.5 8.5-2 5-5 2 2-5 5-2Z"></path></svg><span>Navigate</span></button>
    <button type="button" :aria-pressed="props.activePanel === 'layers'" title="Layers" @click="emit('panel', 'layers')"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m12 3 9 5-9 5-9-5 9-5Z"></path><path d="m3 12 9 5 9-5M3 16l9 5 9-5"></path></svg><span>Layers</span></button>
    <button type="button" :aria-pressed="props.activePanel === 'tracks'" title="Tracks" @click="emit('panel', 'tracks')"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 18V9M9 18V5M14 18v-7M19 18V3"></path></svg><span>Tracks</span></button>
    <button type="button" :aria-pressed="props.activePanel === 'detail'" title="Detail policy" @click="emit('panel', 'detail')"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 6h16M4 12h10M4 18h7"></path><circle cx="18" cy="12" r="2"></circle><circle cx="15" cy="18" r="2"></circle></svg><span>Detail</span></button>
    <span class="rail-spacer"></span>
    <button type="button" title="Archive source" @click="emit('source')"><svg viewBox="0 0 24 24" aria-hidden="true"><ellipse cx="12" cy="5" rx="8" ry="3"></ellipse><path d="M4 5v7c0 1.7 3.6 3 8 3s8-1.3 8-3V5M4 12v7c0 1.7 3.6 3 8 3s8-1.3 8-3v-7"></path></svg><span>Source</span></button>
    <button type="button" title="Keyboard help" @click="emit('help')"><svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="9"></circle><path d="M9.8 9a2.4 2.4 0 1 1 3.5 2.1c-.9.5-1.3 1-1.3 2M12 17h.01"></path></svg><span>Help</span></button>
  </nav>
</template>
