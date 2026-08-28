<script setup lang="ts">
// biome-ignore-all lint/correctness/noUnusedVariables: Vue template bindings are consumed outside the script AST.
defineProps<{
  text: string;
}>();
</script>

<template>
  <span class="help-tooltip" tabindex="0" role="img" :aria-label="text">
    <span aria-hidden="true">?</span>
    <span class="help-tooltip__content" role="tooltip">{{ text }}</span>
  </span>
</template>
