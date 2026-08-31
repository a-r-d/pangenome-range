<script setup lang="ts">
// biome-ignore-all lint/correctness/noUnusedVariables: Vue template bindings are consumed outside the script AST.
import type {
  FeatureQueryTrace,
  NamedSourcePath,
  NamedTraversalGroup,
  PangenomeArchive,
  RegionTile,
} from "pangenome-range/reader";
import {
  bytesToHex,
  type LocalPattern,
  localTraversalFasta,
  localTraversalSequence,
  namedPathMembershipTsv,
} from "pangenome-range/viewer";
import { computed, onBeforeUnmount, ref, watch } from "vue";

const props = defineProps<{
  pattern: LocalPattern;
  archive?: PangenomeArchive;
  tile?: RegionTile;
  archiveIdentity: string;
}>();
const emit = defineEmits<{
  close: [];
  copy: [value: string, label: string];
  highlight: [pathId?: bigint];
  evidence: [value?: PatternEvidence];
}>();
interface PatternEvidence {
  readonly membership: FeatureQueryTrace;
  readonly catalog: FeatureQueryTrace;
}
const group = ref<NamedTraversalGroup>();
const paths = ref<readonly NamedSourcePath[]>([]);
const filter = ref("");
const loading = ref(false);
const error = ref("");
const membershipTrace = ref<FeatureQueryTrace>();
const catalogTrace = ref<FeatureQueryTrace>();
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
const traversalDigest = computed(() =>
  group.value === undefined ? "" : bytesToHex(group.value.traversalDigest),
);
const categorySummary = computed(() => {
  const biologicalLabels = new Set<string>();
  let technical = 0;
  let unknown = 0;
  for (const membership of group.value?.memberships ?? []) {
    const path = pathById.value.get(membership.pathId);
    if (path?.sense === "haplotype") biologicalLabels.add(path.sample);
    else if (path?.sense === "reference" || path?.sense === "generic")
      technical += 1;
    else unknown += 1;
  }
  return { biologicalLabels: biologicalLabels.size, technical, unknown };
});
const tsv = computed(() => {
  if (props.tile === undefined || group.value === undefined) return "";
  return namedPathMembershipTsv({
    tile: props.tile,
    traversalDigest: traversalDigest.value,
    group: group.value,
    paths: paths.value,
  });
});
const sequenceExport = computed<{
  readonly sequence: string;
  readonly fasta: string;
  readonly error: string;
}>(() => {
  if (props.tile === undefined) return { sequence: "", fasta: "", error: "" };
  try {
    const sequence = localTraversalSequence(
      props.tile,
      props.pattern.orientedNodes,
    );
    const fasta =
      group.value === undefined
        ? ""
        : localTraversalFasta({
            archiveIdentity: props.archiveIdentity,
            tile: props.tile,
            traversalDigest: traversalDigest.value,
            occurrenceWeight: group.value.occurrenceWeight,
            orientedNodes: props.pattern.orientedNodes,
          });
    return { sequence, fasta, error: "" };
  } catch (cause) {
    return {
      sequence: "",
      fasta: "",
      error: cause instanceof Error ? cause.message : String(cause),
    };
  }
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
    loading.value = false;
    membershipTrace.value = undefined;
    catalogTrace.value = undefined;
    emit("highlight", undefined);
    emit("evidence", undefined);
    if (
      archive === undefined ||
      tile === undefined ||
      !archive.capabilities().pathMembership
    ) {
      return;
    }
    loading.value = true;
    try {
      const groups = await archive.tilePathMemberships(tile, {
        signal,
        trace: (value) => {
          membershipTrace.value = value;
        },
      });
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
      const resolved = await archive.pathsByIds(pathIds, {
        signal,
        trace: (value) => {
          catalogTrace.value = value;
        },
      });
      const records: NamedSourcePath[] = [];
      for (const [index, path] of resolved.entries()) {
        if (path === undefined)
          throw new Error(`Path ${pathIds[index]} is absent from the catalog`);
        records.push(path);
      }
      if (current !== operation) return;
      group.value = matched;
      paths.value = records;
      if (
        membershipTrace.value !== undefined &&
        catalogTrace.value !== undefined
      )
        emit("evidence", {
          membership: membershipTrace.value,
          catalog: catalogTrace.value,
        });
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

function download(content: string, filename: string, type: string): void {
  if (content.length === 0) return;
  const url = URL.createObjectURL(new Blob([content], { type }));
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  anchor.click();
  URL.revokeObjectURL(url);
}
</script>

<template>
  <aside class="browser-inspector" aria-label="Pattern inspector">
    <header><span>Local traversal pattern</span><button type="button" aria-label="Close inspector" @click="emit('close')">×</button></header>
    <h2>{{ pattern.id }} × {{ pattern.weight.toLocaleString() }}</h2>
    <p class="inspector-warning">Tile-local traversal evidence from {{ pattern.tileKey }}. Occurrence weight is tile-local and is not an allele frequency. Named path fragments are not stitched across tiles.</p>
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
      <p class="membership-summary" data-testid="path-category-summary">
        {{ categorySummary.biologicalLabels.toLocaleString() }} haplotype source labels ·
        {{ categorySummary.technical.toLocaleString() }} reference/generic path records
        <template v-if="categorySummary.unknown > 0"> · {{ categorySummary.unknown.toLocaleString() }} unknown records</template>
      </p>
      <div class="inspector-actions">
        <button type="button" data-testid="copy-path-list" @click="emit('copy', tsv, 'Path list')">Copy path list</button>
        <button type="button" data-testid="download-path-list" @click="download(tsv, `named-paths-${traversalDigest}.tsv`, 'text/tab-separated-values')">Download TSV</button>
        <button type="button" data-testid="copy-local-sequence" :disabled="sequenceExport.sequence.length === 0" @click="emit('copy', sequenceExport.sequence, 'Local traversal sequence')">Copy local sequence</button>
        <button type="button" data-testid="download-local-fasta" :disabled="sequenceExport.fasta.length === 0" @click="download(sequenceExport.fasta, `local-traversal-${traversalDigest}.fa`, 'text/x-fasta')">Download FASTA</button>
      </div>
      <p v-if="sequenceExport.error" class="inspector-warning" data-testid="sequence-export-error">Local sequence export unavailable: {{ sequenceExport.error }}</p>
      <input v-model="filter" type="search" placeholder="Filter sample or path name" aria-label="Filter named source paths" />
      <ul>
        <li v-for="membership in filteredMemberships" :key="`${membership.pathId}:${membership.reversedRelativeToGroup}`">
          <template v-if="pathById.get(membership.pathId)">
            <strong>{{ pathById.get(membership.pathId)?.sample }}</strong>
            <span>{{ pathById.get(membership.pathId)?.canonicalName }}</span>
            <small>{{ pathById.get(membership.pathId)?.sense }} path · haplotype {{ pathById.get(membership.pathId)?.haplotype }}, fragment {{ pathById.get(membership.pathId)?.fragment }}, multiplicity {{ membership.multiplicity }}, {{ membership.reversedRelativeToGroup ? 'reverse' : 'forward' }}</small>
            <button type="button" @click="emit('copy', pathById.get(membership.pathId)?.canonicalName ?? '', 'Path name')">Copy path name</button>
            <button type="button" @click="emit('highlight', membership.pathId)">Highlight path</button>
          </template>
        </li>
      </ul>
    </template>
    <p v-else class="inspector-warning">This archive has anonymous tile-local evidence only; it does not contain named source-path membership.</p>
  </aside>
</template>
