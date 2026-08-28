<script setup lang="ts">
// biome-ignore-all lint/correctness/noUnusedVariables: Vue template bindings are consumed outside the script AST.
import type {
  LocalPattern,
  TubeMapEdge,
  TubeMapModel,
  TubeMapNode,
  TubeMapSourceTile,
} from "pangenome-range/viewer";
import { withBase } from "vitepress";
import { onMounted, shallowRef } from "vue";
import TubeMapView from "./TubeMapView.vue";
import type { BrowserSelection, GraphOptions } from "./types";
import "./browser.css";

interface GoldenNode
  extends Omit<TubeMapNode, "id" | "sourceTile" | "collapsedMembers"> {
  id: string;
  sourceTile: string;
  collapsedMembers?: readonly string[];
}

interface GoldenPattern
  extends Omit<LocalPattern, "weight" | "orientedNodes" | "source"> {
  weight: string;
  orientedNodes: readonly string[];
}

interface GoldenModel extends Omit<TubeMapModel, "nodes" | "patterns"> {
  nodes: readonly GoldenNode[];
  patterns: readonly GoldenPattern[];
}

const model = shallowRef<TubeMapModel>();
const selection = shallowRef<BrowserSelection>();
const options: GraphOptions = {
  patternCount: 4,
  simplifyLinearChains: true,
  showBases: "automatic",
  showTileBoundaries: true,
};

onMounted(async () => {
  const response = await fetch(withBase("/fixtures/tube-map-golden.json"));
  if (!response.ok)
    throw new Error(`golden tube-map fixture returned ${response.status}`);
  model.value = parseGoldenModel((await response.json()) as GoldenModel);
});

function parseGoldenModel(raw: GoldenModel): TubeMapModel {
  const sources = new Map<string, TubeMapSourceTile>(
    raw.tileBoundaries.map((tile, index) => [
      tile.tileKey,
      {
        key: tile.tileKey,
        coreStart: tile.start,
        coreEnd: tile.end,
        archiveOffset: BigInt(index * 4096),
        compressedBytes: 4096,
        uncompressedBytes: 16_384,
      },
    ]),
  );
  const nodes: TubeMapNode[] = raw.nodes.map((node) => ({
    ...node,
    id: BigInt(node.id),
    sourceTile: requireSource(sources, node.sourceTile),
    ...(node.collapsedMembers === undefined
      ? {}
      : { collapsedMembers: node.collapsedMembers.map(BigInt) }),
  }));
  const edges = raw.edges as readonly TubeMapEdge[];
  const patterns: LocalPattern[] = raw.patterns.map((pattern) => ({
    ...pattern,
    weight: BigInt(pattern.weight),
    orientedNodes: pattern.orientedNodes.map(BigInt),
    source: requireSource(sources, pattern.tileKey),
  }));
  return { ...raw, nodes, edges, patterns };
}

function requireSource(
  sources: ReadonlyMap<string, TubeMapSourceTile>,
  key: string,
): TubeMapSourceTile {
  const source = sources.get(key);
  if (source === undefined)
    throw new Error(`golden fixture references unknown tile ${key}`);
  return source;
}
</script>

<template>
  <div class="tube-map-lab">
    <header>
      <strong>Golden tube-map laboratory</strong>
      <span>deterministic two-tile fixture · not a production route</span>
    </header>
    <TubeMapView
      :model="model"
      phase="ready"
      message="Golden fixture ready"
      :options="options"
      :selection="selection"
      @select="selection = $event"
    />
  </div>
</template>

<style>
.tube-map-lab {
  position: fixed;
  inset: 0;
  z-index: 100;
  display: grid;
  grid-template-rows: 48px minmax(0, 1fr);
  background: #f7f8fa;
}
.tube-map-lab > header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 18px;
  border-bottom: 1px solid #d9e0e8;
  color: #172033;
  background: white;
  font: 13px Inter, sans-serif;
}
.tube-map-lab > header span {
  color: #64748b;
}
.tube-map-lab .tube-map-view {
  position: relative;
}
</style>
