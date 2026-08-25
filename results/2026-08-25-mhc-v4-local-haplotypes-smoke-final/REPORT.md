# Fixed-window cloud-layout experiment: 2026-08-25-mhc-v4-local-haplotypes-smoke-final

This is a measured fixed-window archive v4 result for `test-data/mhc-10.gbz` in `single-config-smoke` mode, not a stable-format recommendation. Candidate rows independently passed query-graph comparison and exact weighted tile-local haplotype comparison.

## Baselines

| Baseline | Size | vs GBZ | Build/load | Query p50 / p95 | Storage behavior |
|---|---:|---:|---:|---:|---|
| GBZ | 4511832 B | 1.000x | load 19.412 ms | 2740.1 / 458375.9 us | whole graph must be loaded before interval extraction |
| GBZ-base | 10301440 B | 2.283x | build 472.118 ms; open 0.333 ms | 8570.5 / 1380006.5 us | local SQLite random access; not a static-object range layout |

SQLite page I/O was not observable in this run: strace child exited with exit status: 1

## Fixed-window candidates

The table contains every configuration executed by this mode and uses the 64 KiB coalescing threshold across all query classes. Network p95 is the 20 ms / 300 Mbps profile with dependency rounds enforced.

| Experiment | Archive | vs GBZ | Index | Chunks | p50 / p95 bytes | p95 reads | p95 local | p95 network | Correct |
|---|---:|---:|---:|---:|---:|---:|---:|---:|:---:|
| fixed-w16k-zstd-3 | 3194336 B | 0.708x | 41133 B (1.29%); root 173 B / 10 pages | 304 | 47685 / 941123 | 2 | 1043492.2 us | 46.07 ms | yes |

## Query-size and hard-locus distributions

Results are not collapsed across size classes in `summary.json` or `queries.csv`. For the latency-first Pareto point at 64 KiB coalescing:

| Query group | Count | p50 / p95 / p99 bytes | p50 / p95 / p99 reads | p50 / p95 / p99 local | p95 20 ms network |
|---|---:|---:|---:|---:|---:|
| random-1000 | 10 | 11953 / 31964 / 31964 | 2 / 3 / 3 | 1794.5 / 6724.6 / 6724.6 us | 62.35 ms |
| random-10000 | 10 | 16833 / 45599 / 45599 | 1 / 2 / 2 | 5629.3 / 44254.5 / 44254.5 us | 42.22 ms |
| random-100000 | 10 | 72044 / 163627 / 163627 | 1 / 1 / 1 | 25153.4 / 207015.4 / 207015.4 us | 24.86 ms |
| random-1000000 | 10 | 898544 / 958875 / 958875 | 1 / 1 / 1 | 897910.5 / 1076452.9 / 1076452.9 us | 46.07 ms |

## Arithmetic manifest lookup

The selected archive has a 173-byte header/root bootstrap index and 10 fixed 4 KiB leaf pages. The root contains one manifest per reference path; a reader computes the required page offset directly from the query coordinate. Leaf bytes below count only bytes fetched beyond the 16 KiB bootstrap; a zero means the selected page was already resident in that prefix.

| Query group | p50 / p95 selected pages | p50 / p95 extra leaf bytes | p50 / p95 lookup |
|---|---:|---:|---:|
| random-1000 | 1 / 1 | 4096 / 4096 | 12.5 / 24.1 us |
| random-10000 | 1 / 2 | 0 / 4096 | 12.4 / 32.8 us |
| random-100000 | 1 / 2 | 0 / 0 | 24.0 / 34.5 us |
| random-1000000 | 3 / 3 | 0 / 0 | 87.2 / 131.5 us |

## Encoder construction

The v4 encoder completed in 1660.006 ms while retaining only chunk metadata plus one regional raw/compressed payload at a time. The first payload was written after 11.282 ms. It wrote a 3153203-byte payload spool and exactly 0 occurrence-index bytes. Peak raw/compressed payload buffers were 565982 / 51526 bytes; 0 adaptive splits were required.

| Phase | Wall time |
|---|---:|
| occurrence index (removed) | 0.000 ms |
| upstream regional selection | 670.727 ms |
| regional materialization | 881.051 ms |
| packed binary encoding | 20.589 ms |
| compression | 50.895 ms |


All-vs-Distinct on the first tile `MHC-GRCh38#MHC` 0-16384: All emitted 9 traversals / 504 node visits / 47969 raw JSON bytes in 0.454 ms; Distinct emitted 6 traversals with total weight 9 / 504 weighted node visits / 40047 raw JSON bytes in 0.379 ms. Exact oriented-traversal aggregation matched: **true**.


## Local latency comparison by query size

Local timings use the same deterministic queries. Candidate values use the 64 KiB coalescing threshold. The final column states the direction and factor relative to GBZ-base.

| Query group | GBZ p50 / p95 | GBZ-base p50 / p95 | Candidate p50 / p95 | Candidate vs GBZ-base p95 |
|---|---:|---:|---:|---:|
| random-1000 | 36.2 / 99.2 us | 239.2 / 583.7 us | 1794.5 / 6724.6 us | 11.52x slower |
| random-10000 | 653.7 / 5187.0 us | 2780.7 / 20948.8 us | 5629.3 / 44254.5 us | 2.11x slower |
| random-100000 | 7210.0 / 100955.0 us | 24427.4 / 282705.8 us | 25153.4 / 207015.4 us | 1.37x faster |
| random-1000000 | 413997.3 / 482584.8 us | 1280092.7 / 1500459.0 us | 897910.5 / 1076452.9 us | 1.39x faster |

CPU breakdown for the same selected point (all query classes; microseconds):

| Component | p50 | p90 | p95 | p99 | max |
|---|---:|---:|---:|---:|---:|
| index lookup | 20.6 | 92.4 | 112.3 | 131.5 | 131.5 |
| decompression | 225.5 | 5198.9 | 5458.8 | 5683.2 | 5683.2 |
| binary decode | 122.4 | 12067.6 | 13028.7 | 13542.4 | 13542.4 |
| graph reconstruction | 686.0 | 157633.5 | 168593.7 | 169443.6 | 169443.6 |
| total local query | 8312.0 | 964588.0 | 1043492.2 | 1076452.9 | 1076452.9 |

## Range coalescing

For `fixed-w16k-zstd-3`:

| Gap | p50 / p95 reads | p50 / p95 bytes | p95 20 ms network |
|---:|---:|---:|---:|
| 0 B | 1 / 2 | 47685 / 941123 | 46.07 ms |
| 4096 B | 1 / 2 | 47685 / 941123 | 46.07 ms |
| 16384 B | 1 / 2 | 47685 / 941123 | 46.07 ms |
| 65536 B | 1 / 2 | 47685 / 941123 | 46.07 ms |
| 262144 B | 1 / 2 | 47685 / 941123 | 46.07 ms |
| 1048576 B | 1 / 2 | 47685 / 941123 | 46.07 ms |

## Pareto frontier

The frontier jointly considers archive bytes, p95 reads, bytes, local time, and simulated 20 ms latency. In smoke mode it contains only the single measured candidate and is not a comparative winner.

| Experiment | Archive | p95 reads | p95 bytes | p95 local | p95 20 ms |
|---|---:|---:|---:|---:|---:|
| fixed-w16k-zstd-3 | 3194336 B | 2 | 941123 | 1043492.2 us | 46.07 ms |

## What we learned

- Fixed windows can answer every exercised locus from a small root bootstrap, an optional selected-leaf round, and one parallel data round while preserving exact local graph semantics.
- Query-size and named hard-locus groups remain separate in the retained distributions rather than being collapsed into one average.
- The latency-first Pareto point is `fixed-w16k-zstd-3`: 3194336 bytes (0.708x GBZ), with p95 2 reads, 941123 bytes, and 46.07 ms under the 20 ms profile.
- GBZ-base remains storage-competitive relative to the materialized candidates at 2.283x GBZ, but its measured local p95 was 1380006.5 us and its synchronous SQLite access pattern is poorly matched to static-object range access.

## What failed or remains unresolved

- All requested interval sizes were exercised. The 10,000-query requirement remains deferred to a longer benchmark run.
- Archive expansion is input-specific: fixed headers, the root index, path metadata, and boundary duplication scale differently across fixtures.
- Single-config smoke mode cannot establish a new Pareto winner or attribute deduplication savings without a paired non-deduplicated build.
- Peak RSS is only available as whole-process `VmHWM` (Some(916932) KiB); per-phase construction/query RSS and CPU time are not inferred.
- This materialized representation does not preserve compressed GBWT records; a GBZ-record-preserving branch remains untested.

## What surprised us

Smoke mode isolates correctness and scale behavior of the 16 KiB/zstd-3 archive-v4 candidate; it does not establish a cross-layout winner.

## Next highest-information experiment

Implement the TypeScript HTTP-range reader and rerun the 1 kb and 10 kb workloads in a real browser with explicit cold and warm cache phases. In parallel, generate the GB-scale fixture and use the retained encoder phase/RSS/disk metrics to validate linear scaling before attempting another full layout matrix.
