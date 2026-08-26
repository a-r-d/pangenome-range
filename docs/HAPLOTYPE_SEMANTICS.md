# Haplotype semantics decision

Status: accepted for file-format v1.

## Decision

The only on-disk v1 semantics are
`anonymous-distinct-weighted-tile-paths`. A regional payload retains one
reference path with real identity and coordinate anchoring plus the exact packed
local GBWT records needed to reconstruct anonymous tile-local traversals.

The encoder may use `HaplotypeOutput::All` as an internal correctness oracle,
but it is not a serialized mode and is not exposed by the TypeScript format
model. The current writer does not serialize materialized path tables.

## Identity and query boundary

An anonymous traversal has no sample, contig, global path, or continuation
identifier. Its provenance is the real reference sample and contig plus the
tile's core interval and construction halo. It cannot be stitched to a
traversal from another tile or described as an individual.

A multi-chunk query merges nodes, canonical edges, and the real reference path.
It does not merge anonymous traversals across chunks. Each selected physical
tile is decoded once and returned with its own weighted traversal table.

Weights describe complete canonical traversals through one locally extracted
subgraph. Identical traversals are collapsed by exact packed oriented-node
sequence. If the real reference traversal occurs in that group, one occurrence
is removed before the anonymous weight is reported.

## Record reconstruction

For each local packed GBWT record, readers decode successor positions, mark
local predecessors, start at occurrences with no local predecessor, and follow
successors until they leave the tile. The occurrence anchor identifies the real
reference traversal. Other paths are canonicalized for orientation, sorted,
grouped, and counted.

This work occurs only for selected tiles. It removes the global row-per-visit
index and avoids enumerating every anonymous traversal during archive
construction.

## Correctness

Correctness has two independent gates:

1. The assembled query graph matches a source query for node sequences,
   oriented edges, the real reference traversal, and its requested interval.
2. Every selected archive tile matches a fresh source extraction for the same
   core interval and halo, including every oriented traversal, weight, total
   weight, semantics, and provenance field.

The v1 graph and tile hashes use domain-separated v1 strings shared by Rust and
TypeScript.

## Rejected alternatives

- A global row per GBWT path/node visit is rejected: it wrote about 157 GB of
  SQLite scratch before the first HPRC payload.
- Synthetic sample names or global path IDs are rejected because local
  extraction does not establish that identity.
- Cross-tile anonymous path merging is rejected without a real continuation
  identity.
- Historical named-path and materialized weighted-payload compatibility
  decoders are not part of the pre-release v1 implementation.
