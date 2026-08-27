<script setup lang="ts">
// biome-ignore-all lint/correctness/noUnusedVariables: Vue template bindings are consumed outside the script AST.
import type { LocusHit } from "pangenome-range/reader";
import type { InspectorSelection } from "./types";

const props = defineProps<{ selection: InspectorSelection }>();
const emit = defineEmits<{ close: []; copySequence: [] }>();

function formatCoordinate(value: number): string {
  return new Intl.NumberFormat("en-US", { maximumFractionDigits: 0 }).format(
    value,
  );
}

function formatBytes(value: number | bigint): string {
  const bytes = typeof value === "bigint" ? Number(value) : value;
  if (bytes < 1_024) return `${bytes} B`;
  if (bytes < 1_048_576) return `${(bytes / 1_024).toFixed(1)} KiB`;
  return `${(bytes / 1_048_576).toFixed(1)} MiB`;
}

function formatStrand(value: LocusHit["strand"]): string {
  return value === "forward" ? "+" : value === "reverse" ? "−" : "unknown";
}
</script>

<template>
  <aside v-if="props.selection.kind !== 'archive'" class="right-panel" aria-label="Selection inspector">
    <header><span>Inspector</span><button type="button" aria-label="Close inspector" @click="emit('close')">Close</button></header>
    <section v-if="props.selection.kind === 'node'"><p class="inspector-kind">Selected node</p><h2>N{{ props.selection.node.id.toString() }}</h2><p>Reference-anchored graph node</p><dl><div><dt>Length</dt><dd>{{ props.selection.node.sequenceLength }} bp</dd></div><div><dt>Orientation</dt><dd>{{ props.selection.node.reverse ? 'Reverse' : 'Forward' }}</dd></div><div><dt>Topology</dt><dd>{{ props.selection.node.branchKind }}</dd></div><div><dt>Neighbors</dt><dd>{{ props.selection.incoming.length }} in · {{ props.selection.outgoing.length }} out</dd></div><div><dt>Traversal evidence</dt><dd>{{ props.selection.localTraversalWeights.length }} weighted</dd></div><div><dt>Source tiles</dt><dd>{{ props.selection.node.sourceTiles.length }}</dd></div></dl><h3>Sequence</h3><code class="sequence">{{ props.selection.node.sequence.slice(0, 180) }}{{ props.selection.node.sequence.length > 180 ? '…' : '' }}</code><button type="button" class="primary-button" @click="emit('copySequence')">Copy sequence</button></section>
    <section v-else-if="props.selection.kind === 'edge'"><p class="inspector-kind">Selected edge</p><h2>{{ props.selection.edge.from.toString() }} → {{ props.selection.edge.to.toString() }}</h2><dl><div><dt>Topology</dt><dd>{{ props.selection.edge.classification }}</dd></div><div><dt>Orientation</dt><dd>{{ props.selection.edge.fromReverse ? 'reverse' : 'forward' }} → {{ props.selection.edge.toReverse ? 'reverse' : 'forward' }}</dd></div><div><dt>Source tiles</dt><dd>{{ props.selection.edge.sourceTiles.length }}</dd></div></dl></section>
    <section v-else-if="props.selection.kind === 'traversal'"><p class="inspector-kind">Local traversal evidence</p><h2>Weight {{ props.selection.traversal.weight.toString() }}</h2><p class="explanation">Anonymous evidence from one source tile. It is not a named individual and is never stitched across tiles.</p><dl><div><dt>Core interval</dt><dd>{{ formatCoordinate(props.selection.traversal.tileStart) }}–{{ formatCoordinate(props.selection.traversal.tileEnd) }}</dd></div><div><dt>Oriented nodes</dt><dd>{{ props.selection.traversal.orientedNodes.length }}</dd></div><div><dt>Compressed</dt><dd>{{ formatBytes(props.selection.traversal.source.compressedBytes) }}</dd></div><div><dt>Decoded</dt><dd>{{ formatBytes(props.selection.traversal.source.uncompressedBytes) }}</dd></div></dl></section>
    <section v-else-if="props.selection.kind === 'summary'"><p class="inspector-kind">Summary bin</p><h2>{{ props.selection.bin.reference.contig }}:{{ formatCoordinate(props.selection.bin.reference.start) }}–{{ formatCoordinate(props.selection.bin.reference.end) }}</h2><dl><div><dt>Full bin</dt><dd>{{ formatCoordinate(props.selection.bin.fullBinStart) }}–{{ formatCoordinate(props.selection.bin.fullBinEnd) }}</dd></div><div><dt>Query coverage</dt><dd>{{ Math.round(props.selection.bin.coverageFraction * 100) }}%</dd></div><div><dt>Regional tiles</dt><dd>{{ props.selection.bin.tileCount.toString() }}</dd></div><div><dt>Encoded bytes</dt><dd>{{ formatBytes(props.selection.bin.encodedBytes) }}</dd></div><div><dt>Decoded bytes</dt><dd>{{ formatBytes(props.selection.bin.decodedBytes) }}</dd></div><div><dt>Node records</dt><dd>{{ props.selection.bin.nodeRecords.toString() }}</dd></div><div><dt>Edge records</dt><dd>{{ props.selection.bin.edgeRecords.toString() }}</dd></div><div><dt>Occurrences</dt><dd>{{ props.selection.bin.occurrences.toString() }}</dd></div></dl><p class="explanation">Counters describe the complete underlying bin. They are not clipped-interval counts, variants, frequencies, or people.</p></section>
    <section v-else-if="props.selection.kind === 'locus'"><p class="inspector-kind">Selected locus</p><h2>{{ props.selection.hit.displayName }}</h2><p>{{ props.selection.hit.featureType }}</p><dl><div v-if="props.selection.matchedAlias"><dt>Matched alias</dt><dd>{{ props.selection.hit.matchedName }}</dd></div><div><dt>Stable ID</dt><dd>{{ props.selection.hit.stableId }}</dd></div><div><dt>Strand</dt><dd>{{ formatStrand(props.selection.hit.strand) }}</dd></div><div><dt>Reference</dt><dd>{{ props.selection.hit.reference.sample }}</dd></div><div><dt>Coordinates</dt><dd>{{ props.selection.hit.reference.contig }}:{{ formatCoordinate(props.selection.hit.reference.start) }}–{{ formatCoordinate(props.selection.hit.reference.end) }}</dd></div></dl></section>
  </aside>
</template>
