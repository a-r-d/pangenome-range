<script setup lang="ts">
// biome-ignore-all lint/correctness/noUnusedVariables: Vue template bindings are consumed outside the script AST.
import type { LocusHit } from "pangenome-range/reader";
import { computed, ref } from "vue";

const props = defineProps<{
  modelValue: string;
  suggestions: readonly LocusHit[];
  activeIndex: number;
  busy: boolean;
  disabled: boolean;
  message?: string;
}>();
const emit = defineEmits<{
  "update:modelValue": [value: string];
  submit: [];
  select: [hit: LocusHit];
  "update:activeIndex": [index: number];
  close: [];
}>();
const input = ref<HTMLInputElement>();
const open = computed(
  () => props.suggestions.length > 0 || Boolean(props.message),
);

function onKeydown(event: KeyboardEvent): void {
  if (event.key === "Escape") {
    event.preventDefault();
    emit("close");
  } else if (event.key === "ArrowDown" && props.suggestions.length > 0) {
    event.preventDefault();
    emit(
      "update:activeIndex",
      (props.activeIndex + 1) % props.suggestions.length,
    );
  } else if (event.key === "ArrowUp" && props.suggestions.length > 0) {
    event.preventDefault();
    emit(
      "update:activeIndex",
      (props.activeIndex - 1 + props.suggestions.length) %
        props.suggestions.length,
    );
  } else if (event.key === "Enter") {
    event.preventDefault();
    const selected = props.suggestions[props.activeIndex];
    if (selected === undefined) emit("submit");
    else emit("select", selected);
  }
}

function coordinate(hit: LocusHit): string {
  return `${hit.reference.sample} ${hit.reference.contig}:${hit.reference.start.toLocaleString()}–${hit.reference.end.toLocaleString()}`;
}

defineExpose({ focus: () => input.value?.focus() });
</script>

<template>
  <div class="location-search">
    <span class="location-search__icon" aria-hidden="true">⌕</span>
    <input
      ref="input"
      :value="modelValue"
      :disabled="disabled"
      aria-label="Search gene or coordinate"
      autocomplete="off"
      spellcheck="false"
      placeholder="Search gene or coordinate"
      @input="emit('update:modelValue', ($event.target as HTMLInputElement).value)"
      @keydown="onKeydown"
    />
    <span v-if="busy" class="location-search__busy" aria-label="Searching"></span>
    <div v-if="open" class="location-search__suggestions" role="listbox">
      <button
        v-for="(hit, index) in suggestions"
        :key="`${hit.stableId}-${index}`"
        type="button"
        role="option"
        :aria-selected="index === activeIndex"
        :class="{ 'is-active': index === activeIndex }"
        @mousedown.prevent="emit('select', hit)"
      >
        <strong>{{ hit.displayName }}</strong>
        <span>{{ hit.featureType }} · {{ hit.stableId }}</span>
        <small>{{ coordinate(hit) }}</small>
      </button>
      <p v-if="message" class="location-search__message">{{ message }}</p>
    </div>
  </div>
</template>
