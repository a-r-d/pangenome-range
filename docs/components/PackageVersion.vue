<script setup lang="ts">
import { PANGENOME_RANGE_API_VERSION } from "pangenome-range";
</script>

<template>
  <p class="package-version">
    Workspace package API: <code>pangenome-range@{{ PANGENOME_RANGE_API_VERSION }}</code>
  </p>
</template>

<style scoped>
.package-version {
  padding: 0.75rem 1rem;
  border: 1px solid var(--vp-c-divider);
  border-radius: 8px;
  background: var(--vp-c-bg-soft);
}
</style>
