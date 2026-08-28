<script setup lang="ts">
// biome-ignore-all lint/correctness/noUnusedVariables: Vue template bindings are consumed outside the script AST.
import type {
  LocusHit,
  OverviewBin,
  RegionPlan,
  RegionQuery,
} from "pangenome-range/reader";
import { computed } from "vue";

const props = defineProps<{
  region?: RegionQuery;
  locus?: LocusHit;
  plan?: RegionPlan;
  bins: readonly OverviewBin[];
}>();

const ticks = computed(() => {
  const region = props.region;
  if (region === undefined) return [];
  return Array.from({ length: 5 }, (_, index) => {
    const fraction = index / 4;
    return {
      left: `${fraction * 100}%`,
      coordinate: Math.round(
        region.start + (region.end - region.start) * fraction,
      ),
    };
  });
});

function left(coordinate: number): string {
  const region = props.region;
  if (region === undefined) return "0%";
  return `${((coordinate - region.start) / Math.max(1, region.end - region.start)) * 100}%`;
}

function width(start: number, end: number): string {
  const region = props.region;
  if (region === undefined) return "0%";
  return `${((end - start) / Math.max(1, region.end - region.start)) * 100}%`;
}
</script>

<template>
  <section class="reference-track" aria-label="Linear reference context">
    <div class="reference-track__heading">
      <strong>{{ region ? `${region.sample} · ${region.contig}` : 'Opening archive…' }}</strong>
      <span v-if="region">{{ region.start.toLocaleString() }}–{{ region.end.toLocaleString() }} · {{ (region.end - region.start).toLocaleString() }} bp</span>
    </div>
    <div class="reference-ruler">
      <div class="reference-ruler__line"></div>
      <span v-for="tick in ticks" :key="tick.coordinate" class="reference-ruler__tick" :style="{ left: tick.left }">
        <i></i><small>{{ tick.coordinate.toLocaleString() }}</small>
      </span>
      <span
        v-for="range in plan?.ranges ?? []"
        :key="range.offset.toString()"
        class="reference-ruler__tile"
        :style="{ left: left(range.coreStart), width: width(range.coreStart, range.coreEnd) }"
        :title="`${range.compressedBytes.toLocaleString()} encoded bytes`"
      ></span>
      <span
        v-if="locus && region"
        class="reference-ruler__locus"
        :style="{ left: left(locus.reference.start), width: width(locus.reference.start, locus.reference.end) }"
      >{{ locus.displayName }} {{ locus.strand === 'reverse' ? '←' : '→' }}</span>
    </div>
    <div v-if="bins.length >= 4" class="complexity-strip" aria-label="Archive complexity summary">
      <span
        v-for="bin in bins"
        :key="`${bin.fullBinStart}-${bin.fullBinEnd}`"
        :style="{ flexGrow: Math.max(1, bin.reference.end - bin.reference.start), opacity: String(Math.max(0.2, Math.min(1, Number(bin.nodeRecords) / 50000))) }"
      ></span>
    </div>
  </section>
</template>
