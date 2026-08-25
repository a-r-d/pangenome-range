# Fixed-window cloud-layout experiment: 2026-08-25-mhc-smoke-c0-final

This is a measured Candidate 0 result for `test-data/mhc-10.gbz` in `single-config-smoke` mode, not a format recommendation. All candidate rows passed node, sequence, edge, path-multiplicity/orientation, and reference-coordinate canonical comparison.

## Baselines

| Baseline | Size | vs GBZ | Build/load | Query p50 / p95 | Storage behavior |
|---|---:|---:|---:|---:|---|
| GBZ | 4511832 B | 1.000x | load 23.760 ms | 2822.3 / 473355.5 us | whole graph must be loaded before interval extraction |
| GBZ-base | 10301440 B | 2.283x | build 471.256 ms; open 0.409 ms | 7017.0 / 1111194.1 us | local SQLite random access; not a static-object range layout |

A representative GBZ-base query issued 92 identifiable `pread64` calls and returned 50516 bytes. The trace contained 12 page-sized reads, 79 repeated 16-byte SQLite header checks, and 91 non-sequential offset transitions. This is process-level syscall evidence, not a cold-cache byte count.

## Fixed-window candidates

The table contains every configuration executed by this mode and uses the 64 KiB coalescing threshold across all query classes. Network p95 is the 20 ms / 300 Mbps profile with dependency rounds enforced.

| Experiment | Archive | vs GBZ | Index | Chunks | p50 / p95 bytes | p95 reads | p95 local | p95 network | Correct |
|---|---:|---:|---:|---:|---:|---:|---:|---:|:---:|
| fixed-w256k-zstd-6-dedup | 4552467 B | 1.009x | 1535 B (0.03%) | 19 | 257373 / 1912107 | 2 | 274904.6 us | 91.99 ms | yes |

## Query-size and hard-locus distributions

Results are not collapsed across size classes in `summary.json` or `queries.csv`. For the latency-first Pareto point at 64 KiB coalescing:

| Query group | Count | p50 / p95 / p99 bytes | p50 / p95 / p99 reads | p50 / p95 / p99 local | p95 20 ms network |
|---|---:|---:|---:|---:|---:|
| random-1000 | 10 | 168189 / 479469 / 479469 | 2 / 2 / 2 | 4738.5 / 29748.8 / 29748.8 us | 53.79 ms |
| random-10000 | 10 | 234888 / 1334117 / 1334117 | 2 / 2 / 2 | 9981.1 / 92335.3 / 92335.3 us | 76.58 ms |
| random-100000 | 10 | 257373 / 1334117 / 1334117 | 2 / 2 / 2 | 12310.9 / 116008.9 / 116008.9 us | 76.58 ms |
| random-1000000 | 10 | 1821578 / 1912107 / 1912107 | 2 / 2 / 2 | 234935.2 / 283680.3 / 283680.3 us | 91.99 ms |

## Local latency comparison by query size

Local timings use the same deterministic queries. Candidate values use the 64 KiB coalescing threshold. The final column states the direction and factor relative to GBZ-base.

| Query group | GBZ p50 / p95 | GBZ-base p50 / p95 | Candidate p50 / p95 | Candidate vs GBZ-base p95 |
|---|---:|---:|---:|---:|
| random-1000 | 47.1 / 151.0 us | 179.1 / 451.1 us | 4738.5 / 29748.8 us | 65.95x slower |
| random-10000 | 750.7 / 5437.7 us | 2234.6 / 15812.8 us | 9981.1 / 92335.3 us | 5.84x slower |
| random-100000 | 7365.0 / 96292.9 us | 19030.2 / 226374.7 us | 12310.9 / 116008.9 us | 1.95x faster |
| random-1000000 | 427075.7 / 475905.7 us | 1058230.0 / 1120227.0 us | 234935.2 / 283680.3 us | 3.95x faster |

CPU breakdown for the same selected point (all query classes; microseconds):

| Component | p50 | p90 | p95 | p99 | max |
|---|---:|---:|---:|---:|---:|
| index lookup | 1.0 | 2.3 | 2.3 | 2.4 | 2.4 |
| decompression | 1221.5 | 10789.9 | 10924.9 | 11402.5 | 11402.5 |
| binary decode | 3643.7 | 41438.4 | 41788.3 | 43096.2 | 43096.2 |
| graph reconstruction | 4265.1 | 167685.8 | 169679.7 | 175407.0 | 175407.0 |
| total local query | 11979.0 | 271419.1 | 274904.6 | 283680.3 | 283680.3 |

## Range coalescing

For `fixed-w256k-zstd-6-dedup`:

| Gap | p50 / p95 reads | p50 / p95 bytes | p95 20 ms network |
|---:|---:|---:|---:|
| 0 B | 2 / 2 | 257373 / 1912107 | 91.99 ms |
| 4096 B | 2 / 2 | 257373 / 1912107 | 91.99 ms |
| 16384 B | 2 / 2 | 257373 / 1912107 | 91.99 ms |
| 65536 B | 2 / 2 | 257373 / 1912107 | 91.99 ms |
| 262144 B | 2 / 2 | 257373 / 1912107 | 91.99 ms |
| 1048576 B | 2 / 2 | 257373 / 1912107 | 91.99 ms |

## Pareto frontier

The frontier jointly considers archive bytes, p95 reads, bytes, local time, and simulated 20 ms latency. In smoke mode it contains only the single measured candidate and is not a comparative winner.

| Experiment | Archive | p95 reads | p95 bytes | p95 local | p95 20 ms |
|---|---:|---:|---:|---:|---:|
| fixed-w256k-zstd-6-dedup | 4552467 B | 2 | 1912107 | 274904.6 us | 91.99 ms |

## What we learned

- Fixed windows can answer every exercised locus from a small bootstrap plus one parallel data round while preserving exact local graph semantics.
- Query-size and named hard-locus groups remain separate in the retained distributions rather than being collapsed into one average.
- The latency-first Pareto point is `fixed-w256k-zstd-6-dedup`: 4552467 bytes (1.009x GBZ), with p95 2 reads, 1912107 bytes, and 91.99 ms under the 20 ms profile.
- GBZ-base remains storage-competitive relative to the materialized candidates at 2.283x GBZ, but its measured local p95 was 1111194.1 us and its synchronous SQLite access pattern is poorly matched to static-object range access.

## What failed or remains unresolved

- All requested interval sizes were exercised. The 10,000-query requirement remains deferred to a longer benchmark run.
- Archive expansion is input-specific: fixed headers, the root index, path metadata, and boundary duplication scale differently across fixtures.
- Single-config smoke mode cannot establish a new Pareto winner or attribute deduplication savings without a paired non-deduplicated build.
- Peak RSS is only available as whole-process `VmHWM` (Some(792804) KiB); per-phase construction/query RSS and CPU time are not inferred.
- This materialized representation does not preserve compressed GBWT records; a GBZ-record-preserving branch remains untested.

## What surprised us

Smoke mode intentionally did not search for a new structural improvement; it isolates correctness and scale behavior of the previously selected layout.

## Next highest-information experiment

If this smoke run passes, use its size and construction evidence to choose a deliberately scoped comparative sweep before moving to a multi-gigabyte chromosome or whole-genome input.
