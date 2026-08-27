<script setup lang="ts">
// biome-ignore-all lint/correctness/noUnusedVariables: Vue template bindings are consumed outside the script AST.
import type {
  ArchiveInfo,
  LocusHit,
  OverviewBin,
  RegionPlan,
  RegionQuery,
} from "pangenome-range/reader";
import { computed, ref } from "vue";

const props = defineProps<{
  title: string;
  coordinate: string;
  region?: RegionQuery;
  locus?: LocusHit;
  plan?: RegionPlan;
  bins: readonly OverviewBin[];
  archiveInfo?: ArchiveInfo;
  referenceCount: number;
  skeleton: boolean;
  canOpenDetail: boolean;
  detailReason: string;
  archiveSize: string;
  baseBinSpan: string;
  scale: string;
}>();

const emit = defineEmits<{
  openDetail: [];
  wheel: [event: WheelEvent];
  pointerdown: [event: PointerEvent];
  pointermove: [event: PointerEvent];
  pointerup: [event: PointerEvent];
  copyCoordinate: [];
}>();

const canvas = ref<HTMLCanvasElement>();
defineExpose({ getCanvas: () => canvas.value });

const hotspotStyle = computed(() => {
  if (props.region === undefined || props.locus === undefined) return undefined;
  const span = Math.max(1, props.region.end - props.region.start);
  const left =
    ((props.locus.reference.start - props.region.start) / span) * 100;
  const width =
    ((props.locus.reference.end - props.locus.reference.start) / span) * 100;
  return {
    left: `${Math.max(0, Math.min(100, left))}%`,
    width: `${Math.max(0.8, Math.min(100 - left, width))}%`,
  };
});
</script>

<template>
  <section class="overview-workspace" aria-label="Regional overview viewport">
    <header class="locus-heading">
      <div><p class="eyebrow">Regional orientation</p><h1>{{ props.title }}</h1><p>{{ props.coordinate }}</p></div>
      <span class="mode-label">Regional overview<small>Summary and directory only</small></span>
    </header>
    <div class="overview-card">
      <div class="overview-intro">
        <div><span>{{ props.region?.contig ?? 'Reference' }}</span><h2>Move through the pangenome as a continuous landscape.</h2></div>
        <div class="overview-actions"><button type="button" class="quiet-button" @click="emit('copyCoordinate')">Copy coordinate</button><button type="button" class="primary-button" :disabled="!props.canOpenDetail" @click="emit('openDetail')">Open detailed graph →</button></div>
      </div>
      <div class="chromosome-ribbon" aria-label="Regional payload ribbon">
        <i v-for="range in props.plan?.ranges ?? []" :key="range.offset.toString()"></i>
        <span v-if="hotspotStyle" :style="hotspotStyle"><b>{{ props.locus?.displayName }}</b></span>
      </div>
      <div class="overview-title">
        <div><p class="eyebrow">Multiscale summary</p><h2>Pangenome complexity</h2><p>Archive tile-record estimates with exact transfer planning</p></div>
        <div class="legend"><span><i class="topology"></i>Topology</span><span><i class="traversal"></i>Traversal evidence</span><span><i class="transfer"></i>Transfer cost</span></div>
      </div>
      <div class="summary-stage" :class="{ skeleton: props.skeleton }">
        <canvas
          ref="canvas"
          aria-label="Composite regional overview; drag to pan, wheel to zoom, click a bin to inspect"
          tabindex="0"
          @wheel.prevent="emit('wheel', $event)"
          @pointerdown="emit('pointerdown', $event)"
          @pointermove="emit('pointermove', $event)"
          @pointerup="emit('pointerup', $event)"
          @pointercancel="emit('pointerup', $event)"
        ></canvas>
        <div class="summary-caption"><span>{{ props.bins.length }} summary bin{{ props.bins.length === 1 ? '' : 's' }} · {{ props.plan?.selectedChunks ?? 0 }} graph tile{{ props.plan?.selectedChunks === 1 ? '' : 's' }} planned</span><span v-if="props.bins[0]">{{ props.scale }} visible-bin scale · level {{ props.bins[0].level }} · {{ props.baseBinSpan }}/bin<span v-if="props.bins[0].coverageFraction < 1"> · {{ Math.round(props.bins[0].coverageFraction * 100) }}% edge-bin coverage</span></span></div>
      </div>
      <div class="overview-next-step"><div><span>Recommended next step</span><strong>{{ props.detailReason }}</strong><p>The highlighted locus opens as a bounded graph query; the overview itself fetched no graph payloads.</p></div><button type="button" class="primary-button" :disabled="!props.canOpenDetail" @click="emit('openDetail')">Explore {{ props.locus?.displayName ?? props.title }}</button></div>
      <div class="stat-grid"><article><strong>{{ props.archiveInfo?.namedLoci.recordCount.toString() ?? '0' }}</strong><span>named loci</span></article><article><strong>{{ props.referenceCount }}</strong><span>reference paths</span></article><article><strong>{{ props.baseBinSpan }}</strong><span>finest summary bin</span></article><article><strong>{{ props.archiveSize }}</strong><span>archive size</span></article></div>
    </div>
  </section>
</template>
