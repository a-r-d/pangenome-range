<script setup lang="ts">
// biome-ignore-all lint/correctness/noUnusedVariables: Vue template bindings are consumed outside the script AST.
import type {
  NamedSourcePath,
  NamedTraversalGroup,
  PangenomeArchive,
  RegionTile,
} from "pangenome-range/reader";
import type { LocalPattern } from "pangenome-range/viewer";
import { computed, onBeforeUnmount, ref, watch } from "vue";

const props = defineProps<{
  pattern: LocalPattern;
  archive?: PangenomeArchive;
  tile?: RegionTile;
}>();
const emit = defineEmits<{
  close: [];
  copy: [value: string];
  highlight: [pathId?: bigint];
}>();
const group = ref<NamedTraversalGroup>();
const paths = ref<readonly NamedSourcePath[]>([]);
const filter = ref("");
const loading = ref(false);
const error = ref("");
let operation = 0;
let controller: AbortController | undefined;

const pathById = computed(
  () => new Map(paths.value.map((path) => [path.pathId, path])),
);
const filteredMemberships = computed(() => {
  const value = filter.value.trim().toLocaleLowerCase();
  const memberships = group.value?.memberships ?? [];
  if (value.length === 0) return memberships;
  return memberships.filter((membership) => {
    const path = pathById.value.get(membership.pathId);
    return [path?.sample, path?.canonicalName]
      .filter((item) => item !== undefined)
      .some((item) => item.toLocaleLowerCase().includes(value));
  });
});

watch(
  () => [props.archive, props.tile, props.pattern] as const,
  async ([archive, tile, pattern]) => {
    controller?.abort();
    controller = new AbortController();
    const signal = controller.signal;
    const current = ++operation;
    group.value = undefined;
    paths.value = [];
    error.value = "";
    emit("highlight", undefined);
    if (
      archive === undefined ||
      tile === undefined ||
      !archive.capabilities().pathMembership
    ) {
      return;
    }
    loading.value = true;
    try {
      const groups = await archive.tilePathMemberships(tile, { signal });
      const matched = groups.find(
        (candidate) =>
          candidate.occurrenceWeight === pattern.weight &&
          sameHandles(candidate.orientedNodes, pattern.orientedNodes),
      );
      if (matched === undefined)
        throw new Error("Named membership has no matching local traversal");
      const seen = new Set<bigint>();
      for (const membership of matched.memberships) {
        seen.add(membership.pathId);
      }
      const pathIds = [...seen];
      const resolved = await archive.pathsByIds(pathIds, { signal });
      const records: NamedSourcePath[] = [];
      for (const [index, path] of resolved.entries()) {
        if (path === undefined)
          throw new Error(`Path ${pathIds[index]} is absent from the catalog`);
        records.push(path);
      }
      if (current !== operation) return;
      group.value = matched;
      paths.value = records;
    } catch (cause) {
      if (current === operation)
        error.value = cause instanceof Error ? cause.message : String(cause);
    } finally {
      if (current === operation) loading.value = false;
    }
  },
  { immediate: true },
);

onBeforeUnmount(() => controller?.abort());

function sameHandles(
  left: BigUint64Array | undefined,
  right: readonly bigint[],
): boolean {
  if (left === undefined || left.length !== right.length) return false;
  return left.every((handle, index) => handle === right[index]);
}
</script>

<template>
  <aside class="browser-inspector" aria-label="Pattern inspector">
    <header><span>Local traversal pattern</span><button type="button" aria-label="Close inspector" @click="emit('close')">×</button></header>
    <h2>{{ pattern.id }} × {{ pattern.weight.toLocaleString() }}</h2>
    <p class="inspector-warning">Tile-local traversal evidence from {{ pattern.tileKey }}. Named source paths remain fragment-aware and are highlighted only by canonical path ID.</p>
    <dl>
      <div><dt>Source tile</dt><dd>{{ pattern.tileKey }}</dd></div>
      <div><dt>Core interval</dt><dd>{{ pattern.tileStart.toLocaleString() }}–{{ pattern.tileEnd.toLocaleString() }}</dd></div>
      <div><dt>Node visits</dt><dd>{{ pattern.orientedNodes.length.toLocaleString() }}</dd></div>
      <div><dt>Occurrence weight</dt><dd>{{ pattern.weight.toLocaleString() }}</dd></div>
      <div v-if="group"><dt>Unique named paths</dt><dd>{{ group.uniquePathCount.toLocaleString() }}</dd></div>
    </dl>
    <p v-if="loading">Loading named source paths…</p>
    <p v-else-if="error" class="inspector-warning">{{ error }}</p>
    <template v-else-if="group">
      <h3>Named source paths</h3>
      <input v-model="filter" type="search" placeholder="Filter sample or path name" aria-label="Filter named source paths" />
      <ul>
        <li v-for="membership in filteredMemberships" :key="`${membership.pathId}:${membership.reversedRelativeToGroup}`">
          <template v-if="pathById.get(membership.pathId)">
            <strong>{{ pathById.get(membership.pathId)?.sample }}</strong>
            <span>{{ pathById.get(membership.pathId)?.canonicalName }}</span>
            <small>haplotype {{ pathById.get(membership.pathId)?.haplotype }}, fragment {{ pathById.get(membership.pathId)?.fragment }}, multiplicity {{ membership.multiplicity }}, {{ membership.reversedRelativeToGroup ? 'reverse' : 'forward' }}</small>
            <button type="button" @click="emit('copy', pathById.get(membership.pathId)?.canonicalName ?? '')">Copy path name</button>
            <button type="button" @click="emit('highlight', membership.pathId)">Highlight path</button>
          </template>
        </li>
      </ul>
    </template>
    <p v-else class="inspector-warning">This archive has anonymous tile-local evidence only; it does not contain named source-path membership.</p>
  </aside>
</template>
