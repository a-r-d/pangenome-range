# Viewer format gaps

This file contains only product needs that cannot be represented faithfully by
the existing metadata, named-locus, summary, directory, or regional APIs.

## Exact per-node reference-coordinate anchors

The regional payload preserves the real reference traversal, node lengths,
orientation, tile core interval, and reference identity, but it does not carry
an exact genomic start/end for every reference node. The viewer therefore
places reference nodes in true traversal order and distributes their sequence
lengths across the known regional interval. The ruler and queried interval are
exact; individual node-to-coordinate placement is reference-anchored but may
be approximate.

A future solution should add compact, monotone reference anchors or a tested
arithmetic mapping with measured archive, construction, and range cost. It must
not repeat strings or create a row per haplotype visit. Until then the UI does
not claim exact per-node genomic coordinates.

## Biological variant classification

Topology supports a visual, layout-level distinction between anchored
insertion-like branches, reference bypass/deletion-like edges, inversions, and
unanchored components. The format does not declare normalized variant records,
alleles, genotypes, or allele frequencies. The explorer therefore labels these
as topology classes, not biological variant calls.

Adding normalized variant annotation would require an explicit optional
extension with source/provenance, normalization rules, semantic tests, byte
cost, and both Rust/TypeScript decoders. It is not required for the current
graph explorer.

## Summary granularity at gene scale

The canonical v1 base summary span is `regional_window_size * 64` (1,048,576
bp for the default 16 KiB window). It is compact at whole-HPRC scale, but a
typical 10-100 kb gene query can intersect only part of one base bin. The
stored counters are still exact for that complete bin; only the display-policy
estimate is coverage-prorated.

This tranche does not change the on-disk default. The retained whole-HPRC run
measured only 241,798 encoded extension-body bytes for 591 summary series and
8,017 bins, while the new exact directory planner already supplies payload
count and byte cost without a format change. Coarser 4x, 16x, and 64x base bins
would further reduce an already negligible index while making gene-scale
estimates materially less local. A finer pyramid could improve overview shape,
but must be benchmarked by re-encoding the exact retained source and comparing
construction wall/RSS, extension bytes, range bytes, and gene/100 kb/1 Mb
workloads before changing v1. The large source and archive are not present in
this checkout, so no synthetic result is promoted as whole-HPRC evidence.

## Intentionally absent, not gaps

- Stable arbitrary sample/path identity is absent by design. Anonymous local
  traversals must not be stitched across tiles.
- Allele frequency and individual counts cannot be inferred from traversal
  weights or summary occurrences.
- Per-request timestamps are instrumentation, not archive data; they belong in
  the reader/source trace rather than a file-format revision.
- Adjacent prefetch metadata is unnecessary because directory and summary APIs
  already expose the information needed for a client policy.
