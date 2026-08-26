---
layout: page
aside: false
---

<script setup>
import PangenomeDemo from './components/PangenomeDemo.vue'
</script>

<PangenomeDemo />

The bundled archive is a deterministic synthetic two-node conformance fixture,
so the deployed page is always useful without an external service. It proves
the complete reader/viewer path and exact range behavior; it is not a
population-scale performance claim. Supply a content-addressed remote archive
at build time with `VITE_PANGENOME_RANGE_DEMO_ARCHIVE_URL`, paste a compatible
URL, or choose a local `.pngr` file to inspect richer data.
