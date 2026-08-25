---
layout: home

hero:
  name: pangenome-range
  text: Regional pangenome queries from one static object
  tagline: A Rust encoder and portable TypeScript reader/viewer designed for exact HTTP byte-range access without a custom query server.
  actions:
    - theme: brand
      text: Architecture
      link: /ARCHITECTURE
    - theme: alt
      text: Demo status
      link: /demo

features:
  - title: Static-object delivery
    details: Immutable .pngr archives are designed for ordinary HTTP origins, object stores, and CDNs.
  - title: Native construction
    details: The Rust CLI owns encoding, verification, and reproducible research measurements.
  - title: Portable access
    details: The private TypeScript workspace establishes reader, viewer, and Node entry points while decoding remains under development.
---

<script setup>
import PackageVersion from './components/PackageVersion.vue'
</script>

<PackageVersion />

The current archive v3 is a research prototype, not a stable interchange format.
Its known large-input occurrence-index design is rejected and documented in the
[optimization log](./OPTIMIZATION_LOG.md). The browser decoder and interactive
viewer are not implemented yet; this site currently proves the product and
package boundaries only.

Start with the [architecture](./ARCHITECTURE.md), the
[fixed-window archive description](./FIXED_WINDOW_ARCHIVE.md), and the
[benchmark definitions](./BENCHMARKS.md).
