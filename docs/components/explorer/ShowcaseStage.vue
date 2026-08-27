<script setup lang="ts">
// biome-ignore-all lint/correctness/noUnusedVariables: Vue template bindings are consumed outside the script AST.
const props = defineProps<{
  locus: string;
  archiveSize: string;
  namedLoci: string;
  referencePaths: string;
  detailTransfer: string;
  ready: boolean;
}>();

const emit = defineEmits<{
  openDetail: [];
  openOverview: [];
  openSearch: [];
}>();
</script>

<template>
  <section class="showcase-stage" aria-label="Pangenome Explorer showcase">
    <div class="showcase-copy">
      <p class="eyebrow"><span></span>Browser-native pangenome exploration</p>
      <h1>Explore human<br />pangenomes like<br /><em>a living map.</em></h1>
      <p class="showcase-lede">Search a gene or coordinate, skim regional complexity instantly, then stream only the graph detail worth seeing.</p>
      <div class="showcase-actions">
        <button type="button" class="primary-button showcase-primary" :disabled="!props.ready" @click="emit('openDetail')">Explore {{ props.locus }} <span>→</span></button>
        <button type="button" class="secondary-button" :disabled="!props.ready" @click="emit('openOverview')">Regional overview</button>
        <button type="button" class="quiet-button" @click="emit('openSearch')">Search another locus <kbd>⌘K</kbd></button>
      </div>
      <div class="showcase-stats" aria-label="Archive highlights">
        <article><strong>{{ props.archiveSize }}</strong><span>immutable archive</span></article>
        <article><strong>{{ props.namedLoci }}</strong><span>named loci</span></article>
        <article><strong>{{ props.referencePaths }}</strong><span>reference paths</span></article>
        <article><strong>{{ props.detailTransfer }}</strong><span>planned detail transfer</span></article>
      </div>
      <p class="showcase-footnote">No query server. No whole-archive download. Exact HTTP byte ranges from one static object.</p>
    </div>
    <div class="showcase-preview-placeholder" aria-hidden="true"></div>
  </section>
</template>
