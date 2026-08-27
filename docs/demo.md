---
layout: page
aside: false
---

<script setup>
import PangenomeDemo from './components/PangenomeDemo.vue'
</script>

<PangenomeDemo />

The deployed page defaults to the content-addressed whole-HPRC GENCODE v50
archive documented in [Hosting an archive](./HOSTING.md). The bundled
deterministic two-node conformance fixture remains available for a fast offline
example. You can also supply a different immutable archive at build time with
`VITE_PANGENOME_RANGE_DEMO_ARCHIVE_URL`, paste a compatible URL, or choose a
local `.pngr` file.
