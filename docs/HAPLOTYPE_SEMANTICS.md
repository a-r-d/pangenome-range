# Haplotype semantics decision

Status: accepted for archive v4.

## Decision

Archive v4 uses `anonymous-distinct-weighted-tile-paths` by default. Each
regional payload contains one reference path with its real sample, contig,
orientation, and coordinate interval, plus a sorted collection of anonymous
tile-local traversals with exact integer multiplicities.

The storage-independent model distinguishes three modes:

- `named-paths-v3`: globally named paths from the legacy research format;
- `anonymous-all-tile-paths`: every anonymous local traversal with weight one;
- `anonymous-distinct-weighted-tile-paths`: exact local traversals collapsed by
  oriented-node sequence with their multiplicities.

The canonical v4 types are `HaplotypeSemantics`, `WeightedTraversal`, and
`CanonicalHaplotypeTile` in `pangenome-range-query`. Reference paths remain in
the graph result; anonymous evidence is returned as an ordered list of source
tiles.

## Identity and query boundary

An anonymous traversal has no sample, contig, path, or continuation identifier.
Its provenance is the reference sample and contig, core interval, and fixed
construction halo of the payload that produced it. It cannot be stitched to a
traversal from another tile or described as an individual.

A multi-chunk query merges nodes, edges, and the real reference path. It does
not merge anonymous traversals across chunks. Each physical tile is returned at
most once, in `(reference sample, reference contig, core start, core end)` order,
so overlapping construction halos cannot double-count a tile's weights.

Weights describe complete traversals through the locally extracted subgraph,
including halo nodes. The core interval determines ownership and provenance;
the halo is context, not a second ownership interval.

## Upstream adapter

`gbz-base` 0.6.1 deliberately does not expose non-reference path identities.
`HaplotypeOutput::All` emits each local path and `Distinct` sorts exact handle
sequences and stores duplicate counts as weights. The path fields are private,
so the adapter consumes the crate's public JSON serialization and immediately
converts it to typed oriented-node vectors. This allocation is bounded by one
active regional extraction and is released after the payload is encoded.

The reference walk is emitted first. In distinct mode its weight may include
anonymous paths identical to the reference. The adapter preserves the real
reference once and records `reference weight - 1` as anonymous evidence for the
same oriented traversal. This makes the anonymous multiset equivalent to `All`
with its one real reference walk removed.

The MHC experiment must prove that aggregating `All` by exact oriented traversal
equals `Distinct`, after applying that reference rule. If that assertion fails,
the encoder must fall back to `anonymous-all-tile-paths`; it must not publish
incorrect weights.

## Correctness

Correctness has two independent gates:

1. The assembled query graph matches a source query for node sequences,
   oriented edges, the reference traversal, and its requested interval.
2. Every selected archive tile matches a fresh source extraction for the same
   core interval and construction halo, including every oriented traversal,
   weight, total weight, semantics code, and provenance field.

The v4 canonical graph and tile hashes use new domain strings. They are not
comparable to the v3 named-path hash.

## Rejected alternatives

- A global row per GBWT path/node visit is rejected: it wrote about 157 GB of
  SQLite scratch before the first HPRC payload.
- Synthetic sample names or global path IDs are rejected because the local
  extractor cannot establish that identity.
- Cross-tile anonymous path merging is rejected without a real continuation
  identity.
