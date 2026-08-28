<script setup lang="ts">
// biome-ignore-all lint/correctness/noUnusedVariables: Vue template bindings are consumed outside the script AST.
import type { TubeMapModel } from "pangenome-range/viewer";
import {
  layoutTubeMap,
  nodeWidth,
  renderTubeMapSvg,
  type TubeMapRenderResult,
} from "pangenome-range/viewer";
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import type { BrowserPhase, BrowserSelection, GraphOptions } from "./types";

const props = defineProps<{
  model?: TubeMapModel;
  phase: BrowserPhase;
  message: string;
  oversizedMessage?: string;
  options: GraphOptions;
  selection?: BrowserSelection;
}>();
const emit = defineEmits<{
  select: [selection?: BrowserSelection];
  metrics: [metrics: { layoutMs: number; svgElements: number }];
  recommended: [];
}>();
const host = ref<HTMLElement>();
const svg = ref<SVGSVGElement>();
const zoom = ref(1);
const panX = ref(0);
const panning = ref(false);
let renderer: TubeMapRenderResult | undefined;
let resizeObserver: ResizeObserver | undefined;
let paintFrame: number | undefined;
let pointer:
  | { id: number; x: number; panX: number; moved: boolean }
  | undefined;

const MIN_ZOOM = 0.2;
const MAX_ZOOM = 5;
const SIDE_PADDING = 72;
const REFERENCE_GAP = 24;
const BUTTON_ZOOM_FACTOR = 1.2;
const WHEEL_LOG_SENSITIVITY = 0.0008;
const MAX_WHEEL_LOG_STEP = 0.08;

async function paint(): Promise<void> {
  await nextTick();
  const model = props.model;
  const element = svg.value;
  const container = host.value;
  if (
    model === undefined ||
    element === undefined ||
    container === undefined ||
    !model.withinDisplayLimits
  ) {
    renderer?.destroy();
    return;
  }
  const started = performance.now();
  const layout = layoutTubeMap(model, {
    width: Math.max(640, container.clientWidth),
    height: Math.max(360, container.clientHeight),
    zoom: zoom.value,
    panX: panX.value,
    showBases: props.options.showBases,
  });
  renderer?.destroy();
  renderer = renderTubeMapSvg(element, layout, {
    selectedNodeKey:
      props.selection?.kind === "node" ? props.selection.node.key : undefined,
    selectedPatternId:
      props.selection?.kind === "pattern"
        ? props.selection.pattern.id
        : undefined,
    showTileBoundaries: props.options.showTileBoundaries,
    onNodeSelect: (key) => {
      const node = model.nodes.find((candidate) => candidate.key === key);
      if (node !== undefined) emit("select", { kind: "node", node });
    },
    onPatternSelect: (id) => {
      const pattern = model.patterns.find((candidate) => candidate.id === id);
      if (pattern !== undefined) emit("select", { kind: "pattern", pattern });
    },
  });
  emit("metrics", {
    layoutMs: performance.now() - started,
    svgElements: renderer.svgElements,
  });
}

function zoomIn(): void {
  zoomBy(BUTTON_ZOOM_FACTOR);
}

function zoomOut(): void {
  zoomBy(1 / BUTTON_ZOOM_FACTOR);
}

function fit(): void {
  const available = Math.max(320, (host.value?.clientWidth ?? 900) - 144);
  const nodes = new Map(
    props.model?.nodes.map((node) => [node.key, node]) ?? [],
  );
  const nominalWidth =
    props.model?.reference.reduce(
      (total, key) =>
        total + nodeWidth(nodes.get(key) ?? { sequenceLength: 1 }) + 24,
      0,
    ) ?? available;
  zoom.value = Math.max(
    MIN_ZOOM,
    Math.min(1, available / Math.max(1, nominalWidth)),
  );
  panX.value = 0;
}

function onWheel(event: WheelEvent): void {
  event.preventDefault();
  if (
    event.ctrlKey ||
    event.metaKey ||
    Math.abs(event.deltaY) > Math.abs(event.deltaX)
  ) {
    const unit =
      event.deltaMode === WheelEvent.DOM_DELTA_LINE
        ? 16
        : event.deltaMode === WheelEvent.DOM_DELTA_PAGE
          ? (host.value?.clientHeight ?? 600)
          : 1;
    const delta = Math.max(-100, Math.min(100, event.deltaY * unit));
    const logStep = Math.max(
      -MAX_WHEEL_LOG_STEP,
      Math.min(MAX_WHEEL_LOG_STEP, -delta * WHEEL_LOG_SENSITIVITY),
    );
    zoomBy(Math.exp(logStep), event.clientX);
  } else {
    panX.value -= event.deltaX;
  }
}

function zoomBy(factor: number, anchorClientX?: number): void {
  const container = host.value;
  const model = props.model;
  if (container === undefined || model === undefined) return;
  const previous = zoom.value;
  const next = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, previous * factor));
  if (next === previous) return;

  const width = Math.max(640, container.clientWidth);
  const anchor =
    anchorClientX === undefined
      ? width / 2
      : anchorClientX - container.getBoundingClientRect().left;
  const nodes = new Map(model.nodes.map((node) => [node.key, node]));
  const nominalWidth = model.reference.reduce(
    (total, key, index) =>
      total +
      nodeWidth(nodes.get(key) ?? { sequenceLength: 1 }) +
      (index === model.reference.length - 1 ? 0 : REFERENCE_GAP),
    0,
  );
  const previousOrigin = Math.max(
    SIDE_PADDING,
    (width - nominalWidth * previous) / 2,
  );
  const nextOrigin = Math.max(SIDE_PADDING, (width - nominalWidth * next) / 2);
  const world = (anchor - previousOrigin - panX.value) / previous;
  panX.value = anchor - nextOrigin - world * next;
  zoom.value = next;
}

function onPointerDown(event: PointerEvent): void {
  if (
    event.button !== 0 ||
    (event.target as Element).closest("[data-node-key], [data-pattern-id]")
  )
    return;
  pointer = {
    id: event.pointerId,
    x: event.clientX,
    panX: panX.value,
    moved: false,
  };
  panning.value = true;
  host.value?.setPointerCapture(event.pointerId);
}

function onPointerMove(event: PointerEvent): void {
  if (pointer?.id !== event.pointerId) return;
  const movement = event.clientX - pointer.x;
  if (Math.abs(movement) > 3) pointer.moved = true;
  panX.value = pointer.panX + movement;
}

function onPointerUp(event: PointerEvent): void {
  if (pointer?.id !== event.pointerId) return;
  const moved = pointer.moved;
  pointer = undefined;
  panning.value = false;
  if (!moved) emit("select", undefined);
}

function onPointerCancel(event: PointerEvent): void {
  if (pointer?.id !== event.pointerId) return;
  pointer = undefined;
  panning.value = false;
}

function onDoubleClick(event: MouseEvent): void {
  if ((event.target as Element).closest("[data-node-key], [data-pattern-id]"))
    return;
  event.preventDefault();
  zoomBy(1.4, event.clientX);
}

function schedulePaint(): void {
  if (paintFrame !== undefined) return;
  paintFrame = requestAnimationFrame(() => {
    paintFrame = undefined;
    void paint();
  });
}

watch(
  () => props.model,
  () => fit(),
);

watch(
  () => [props.model, props.options, props.selection, zoom.value, panX.value],
  schedulePaint,
  { deep: true },
);

onMounted(() => {
  resizeObserver = new ResizeObserver(schedulePaint);
  if (host.value !== undefined) resizeObserver.observe(host.value);
  fit();
});

onBeforeUnmount(() => {
  resizeObserver?.disconnect();
  if (paintFrame !== undefined) cancelAnimationFrame(paintFrame);
  renderer?.destroy();
});

defineExpose({ zoomIn, zoomOut, fit });
</script>

<template>
  <main
    ref="host"
    class="tube-map-view"
    :class="{ 'is-panning': panning }"
    aria-label="Pangenome tube map"
    @wheel="onWheel"
    @pointerdown="onPointerDown"
    @pointermove="onPointerMove"
    @pointerup="onPointerUp"
    @pointercancel="onPointerCancel"
    @lostpointercapture="onPointerCancel"
    @dblclick="onDoubleClick"
  >
    <svg ref="svg" class="tube-map-view__svg"></svg>
    <div v-if="oversizedMessage" class="graph-state graph-state--oversized">
      <strong>This interval is too large for a readable local graph.</strong>
      <p>{{ oversizedMessage }}</p>
      <button type="button" @click="emit('recommended')">Open recommended 40 kb window</button>
    </div>
    <div v-else-if="phase === 'opening' || (phase === 'streaming' && !model)" class="graph-state graph-state--loading">
      <span class="graph-loader"></span>
      <strong>{{ message }}</strong>
      <p>The reference context stays visible while byte ranges arrive.</p>
    </div>
    <div v-else-if="phase === 'error'" class="graph-state graph-state--error">
      <strong>Graph unavailable</strong><p>{{ message }}</p>
    </div>
    <div v-else-if="model && !model.withinDisplayLimits" class="graph-state graph-state--oversized">
      <strong>Local graph still exceeds the display budget.</strong>
      <p>{{ model.displayLimitMessage }}</p>
      <button type="button" @click="emit('recommended')">Open recommended 40 kb window</button>
    </div>
    <div v-if="phase === 'streaming' && model" class="graph-progress">{{ message }}</div>
    <div class="graph-legend" aria-label="Graph legend"><span class="legend-reference"></span>reference <span class="legend-pattern"></span>tile-local weighted patterns <span class="legend-topology"></span>topology</div>
  </main>
</template>
