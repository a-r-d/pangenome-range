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

## Intentionally absent, not gaps

- Stable arbitrary sample/path identity is absent by design. Anonymous local
  traversals must not be stitched across tiles.
- Allele frequency and individual counts cannot be inferred from traversal
  weights or summary occurrences.
- Per-request timestamps are instrumentation, not archive data; they belong in
  the reader/source trace rather than a file-format revision.
- Adjacent prefetch metadata is unnecessary because directory and summary APIs
  already expose the information needed for a client policy.
