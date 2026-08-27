<script setup lang="ts">
// biome-ignore-all lint/correctness/noUnusedVariables: Vue template bindings are consumed outside the script AST.
import { ref } from "vue";
import type { ExplorerPhase } from "./types";

const props = defineProps<{
  command: string;
  paletteOpen: boolean;
  activeDescendant?: string;
  archiveTitle: string;
  archiveSize: string;
  phase: ExplorerPhase;
}>();

const emit = defineEmits<{
  "update:command": [value: string];
  focus: [];
  keydown: [event: KeyboardEvent];
  submit: [];
  copyLink: [];
  toggleTheme: [];
  openSource: [];
  openHome: [];
}>();

const input = ref<HTMLInputElement>();

defineExpose({
  focus: () => input.value?.focus(),
});
</script>

<template>
  <header class="topbar">
    <button class="brand" type="button" aria-label="Pangenome Explorer home" @click="emit('openHome')">
      <span class="brand-mark" aria-hidden="true">P</span>
      <span><strong>pangenome-range</strong><small>Pangenome explorer</small></span>
    </button>
    <form class="command" role="search" @submit.prevent="emit('submit')">
      <svg class="command-icon" viewBox="0 0 24 24" aria-hidden="true"><circle cx="11" cy="11" r="7"></circle><path d="m20 20-4-4"></path></svg>
      <input
        ref="input"
        :value="props.command"
        aria-label="Go to a locus or genomic coordinate"
        aria-autocomplete="list"
        :aria-expanded="props.paletteOpen"
        :aria-activedescendant="props.activeDescendant"
        autocomplete="off"
        placeholder="Search genes, aliases, or coordinates"
        @input="emit('update:command', ($event.currentTarget as HTMLInputElement).value)"
        @focus="emit('focus')"
        @keydown="emit('keydown', $event)"
      />
      <kbd>⌘K</kbd>
      <button class="command-submit" type="submit">Open</button>
    </form>
    <nav class="top-actions" aria-label="Explorer actions">
      <button type="button" class="icon-button" title="Copy region link" aria-label="Copy region link" @click="emit('copyLink')"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M14 3h7v7M10 14 21 3M21 14v5a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5"></path></svg></button>
      <button type="button" class="icon-button" title="Toggle color theme" aria-label="Toggle color theme" @click="emit('toggleTheme')"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3a9 9 0 1 0 9 9c0-.5 0-1-.1-1.5A7 7 0 0 1 12 3Z"></path></svg></button>
      <button type="button" class="source-button" @click="emit('openSource')"><span class="source-dot" :data-state="props.phase"></span><span>{{ props.archiveTitle }}</span><small>{{ props.archiveSize }}</small></button>
    </nav>
  </header>
</template>
