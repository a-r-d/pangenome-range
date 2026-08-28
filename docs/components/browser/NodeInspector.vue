<script setup lang="ts">
// biome-ignore-all lint/correctness/noUnusedVariables: Vue template bindings are consumed outside the script AST.
import type { TubeMapModel, TubeMapNode } from "pangenome-range/viewer";
import { computed } from "vue";

const props = defineProps<{ node: TubeMapNode; model?: TubeMapModel }>();
const emit = defineEmits<{
  close: [];
  copy: [value: string];
  expand: [nodeKey: string];
}>();

const incoming = computed(
  () =>
    props.model?.edges.filter((edge) => edge.to === props.node.key).length ?? 0,
);
const outgoing = computed(
  () =>
    props.model?.edges.filter((edge) => edge.from === props.node.key).length ??
    0,
);
const boundaryIds = computed(() => {
  const members = props.node.collapsedMembers;
  if (members === undefined) return props.node.id.toString();
  return `${members[0]?.toString() ?? "?"} → ${members.at(-1)?.toString() ?? "?"}`;
});
</script>

<template>
  <aside class="browser-inspector" aria-label="Node inspector">
    <header><span>Sequence node</span><button type="button" aria-label="Close inspector" @click="emit('close')">×</button></header>
    <h2>{{ node.collapsedMembers ? `${node.collapsedMembers.length}-node collapsed chain` : `Node ${node.id}` }}</h2>
    <dl>
      <div><dt>{{ node.collapsedMembers ? 'Boundary IDs' : 'Node ID' }}</dt><dd>{{ boundaryIds }}</dd></div>
      <div><dt>Orientation</dt><dd>{{ node.reverse ? 'reverse' : 'forward' }}</dd></div>
      <div><dt>Length</dt><dd>{{ node.sequenceLength.toLocaleString() }} bp</dd></div>
      <div><dt>Neighbors</dt><dd>{{ incoming }} incoming · {{ outgoing }} outgoing</dd></div>
      <div><dt>Source tile</dt><dd>{{ node.sourceTile.key }} · {{ node.sourceTile.coreStart.toLocaleString() }}–{{ node.sourceTile.coreEnd.toLocaleString() }}</dd></div>
    </dl>
    <label>Sequence preview</label>
    <pre>{{ node.sequence }}</pre>
    <button v-if="node.collapsedMembers" type="button" @click="emit('expand', node.key)">Expand chain in graph</button>
    <button type="button" @click="emit('copy', node.sequence)">Copy sequence</button>
  </aside>
</template>
