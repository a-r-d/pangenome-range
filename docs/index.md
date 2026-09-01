---
layout: home

hero:
  name: pangenome-range
  text: Regional pangenome queries from one static object
  tagline: A Rust encoder and portable TypeScript reader/viewer designed for exact HTTP byte-range access without a custom query server.
  actions:
    - theme: brand
      text: Explore named chicken paths
      link: /demo?archive=chicken&locus=IGLL1
    - theme: alt
      text: Open the large human pangenome
      link: /demo?archive=hprc&locus=HLA-B

features:
  - title: Static-object delivery
    details: Immutable .pngr archives are designed for ordinary HTTP origins, object stores, and CDNs.
  - title: Native construction
    details: The Rust CLI owns encoding, verification, and reproducible research measurements.
  - title: Portable access
    details: The public npm package opens local or remote file-format-v1 objects, renders regional tiles, and launches the optional native encoder CLI without coupling it to browser imports.
---

<script setup>
import PackageVersion from './components/PackageVersion.vue'
import RangeReadAnimation from './components/RangeReadAnimation.vue'
</script>

<PackageVersion />

The current file-format v1 is a pre-release research prototype, not a stable
interchange format. Older research objects are intentionally unsupported.
It removes the rejected global occurrence index and exposes anonymous weighted
tile-local haplotypes explicitly; see the
[semantics decision](./HAPLOTYPE_SEMANTICS.md) and
[optimization log](./OPTIMIZATION_LOG.md). The browser reader, decoder, and
bounded Canvas 2D viewer are implemented and exercised through exact range
responses in a built-site Playwright gate. The [demo](./demo.md) also opens
local files and configurable remote archives.

Start with the [range-read walkthrough](./how-range-reads-work.md), the
[normative file-format v1](./FILE_FORMAT_V1.md), the
[architecture](./ARCHITECTURE.md), the
[fixed-window archive description](./FIXED_WINDOW_ARCHIVE.md), and the
[benchmark definitions](./BENCHMARKS.md).

## How a range query works

A `.pngr` object is laid out so a browser can answer a genomic region with a
small bootstrap, one arithmetic directory lookup, and one parallel payload
round. No query server.

<ClientOnly>
  <RangeReadAnimation />
</ClientOnly>
