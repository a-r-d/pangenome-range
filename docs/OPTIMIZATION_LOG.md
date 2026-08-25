# Optimization log

This log records failed or incomplete performance experiments as first-class
evidence. A run belongs here when it changes what we believe about the encoder,
even if it never produces a candidate archive.

## 2026-08-25: HPRC v2.1 whole-genome occurrence-index blow-up

### Goal

Exercise the v3 16 KiB/zstd-3 encoder on a real multi-gigabyte source without
building GBZ-base or repeating the layout sweep.

The source was the HPRC v2.1 Minigraph-Cactus GRCh38 GBZ:

- URL: <https://s3-us-west-2.amazonaws.com/human-pangenomics/pangenomes/freeze/release2/minigraph-cactus/v2.1/hprc-v2.1-mc-grch38/hprc-v2.1-mc-grch38.gbz>
- Size: 5,492,627,216 bytes
- SHA-256: `11d6047f79575ffb83757462484bad134ed20928bd2c8171ec52e35a54976e2b`
- Graph: 139,520,381 nodes, 53,150 paths, 464 haplotypes, and 292
  reference-path fragments

The process had a 18,000,000 KiB virtual-memory limit. Source and scratch were
on `/media/ard/eba76579-d702-4ff0-b5dd-eb503a726a4d`.

### Observed result

The run was intentionally stopped after 47m27s because preprocessing was still
growing and the final archive had not received a single payload byte.

| Measurement | Observed value |
|---|---:|
| Source GBZ | 5,492,627,216 bytes (5.12 GiB) |
| Temporary SQLite occurrence table at stop | 157,105,246,208 bytes (146.32 GiB) |
| Temporary/source ratio at stop | 28.60x |
| Final payload spool | 0 bytes |
| Encoder RSS near stop | 8,417,564 KiB (8.03 GiB) |
| Elapsed time | 2,847 seconds |
| Average temporary-file growth | 52.63 MiB/s |

These are lower bounds on the old design's total cost. The path scan had not
finished, and SQLite had not yet built `visits_by_node`, the second B-tree over
the already enormous `visits` table.

The retained evidence is in
[`results/2026-08-25-hprc-v2.1-grch38-encoder-scale`](../results/2026-08-25-hprc-v2.1-grch38-encoder-scale/REPORT.md).
The 157 GB temporary table and empty spool were deleted after the measurements
were recorded. The downloaded source GBZ was retained outside the repository.

### Attribution

There are two independent scale costs:

1. **Upstream source loading.** A standalone `inspect` invocation, which does
   not run our encoder, peaked at 8,775,216 KiB RSS while
   `simple_sds::serialize::load_from::<GBZ>` deserialized the complete source.
   This is an upstream `simple-sds`/`gbz` full-load behavior that our encoder
   currently chooses to use.
2. **Our occurrence-index design.** `PathOccurrenceIndex::build` scans every
   path and inserts one SQLite row for every node visit. Each row repeats
   `node_id`, `path_id`, `visit_index`, orientation, start, and end; start/end
   are retained even though only reference visits use them. It is
   single-threaded and does all of this before the encoder writes a chunk.

The v3 spool made our additional memory bounded, but the global row-per-visit
SQLite representation made temporary disk and startup time unacceptable.

### Replacement direction

The next prototype must remove `PathOccurrenceIndex` from the normal archive
build. `gbz-base::Subgraph` already decompresses the selected GBWT node records
and can extract local traversals with `HaplotypeOutput::All`. We currently ask
for `HaplotypeOutput::None`, discard that local information, and reconstruct
named paths using the global SQLite table.

The replacement should:

1. Extract topology and haplotype traversals only for the active chunk.
2. Encode those local traversals immediately and release them before advancing.
3. Keep named reference identity and coordinates in the arithmetic manifest.
4. Avoid any source-global occurrence table or sort before the first chunk.

There is a semantics decision to make explicit. The upstream local extractor
preserves traversal multiplicity but exposes non-reference paths anonymously.
The pinned `gbz` 0.7.0 code and current upstream Rust implementation retain
the GBWT document-array samples as opaque bytes and do not expose a locate API,
so a local GBWT occurrence cannot currently be mapped back to its true path ID
without an auxiliary structure.
If browser rendering only needs local haplotype shapes and counts, that is the
preferred compact representation. If per-sample identity is required, add a
separate compact identity sidecar: dictionary-code names once, store integer
identifiers only, delta-code visits, and keep coordinates only for reference
paths. Do not return to a general SQLite row per visit.

### Acceptance gates before another whole-genome run

- MHC canonical correctness remains true for 1 kb, 10 kb, 100 kb, and 1 Mb
  queries.
- The encoder writes its first payload chunk without a source-global path scan.
- Temporary occurrence-index bytes are exactly zero.
- Peak additional encoder RSS remains bounded by the configured raw chunk,
  compression buffers, and small lookup caches; report source-load RSS
  separately.
- Progress reports distinguish source load, path-index construction, chunk
  selection, path extraction, encoding, compression, and final assembly.
- The full HPRC run completes under an explicit scratch budget and preserves
  the selected path-identity semantics.

Reducing the 8.4 GiB source-load RSS is a separate track. Candidate approaches
are a range/memory-mapped GBZ reader, consuming an already-built GBZ-base
database, or upstream locate/cached-GBWT support. None of those should block
removing our 157 GB temporary occurrence table first.
