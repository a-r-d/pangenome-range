# Fixed-window cloud-layout experiment: 2026-08-25-mhc-two-level-w16k-smoke-c1

This is a measured fixed-window archive v2 result for `test-data/mhc-10.gbz` in `single-config-smoke` mode, not a format recommendation. All candidate rows passed node, sequence, edge, path-multiplicity/orientation, and reference-coordinate canonical comparison.

## Baselines

| Baseline | Size | vs GBZ | Build/load | Query p50 / p95 | Storage behavior |
|---|---:|---:|---:|---:|---|
| GBZ | 4511832 B | 1.000x | load 18.160 ms | 2479.8 / 441617.6 us | whole graph must be loaded before interval extraction |
| GBZ-base | 10301440 B | 2.283x | build 466.655 ms; open 0.372 ms | 6760.2 / 1072501.7 us | local SQLite random access; not a static-object range layout |

A representative GBZ-base query issued 92 identifiable `pread64` calls and returned 50516 bytes. The trace contained 12 page-sized reads, 79 repeated 16-byte SQLite header checks, and 91 non-sequential offset transitions. This is process-level syscall evidence, not a cold-cache byte count.

## Fixed-window candidates

The table contains every configuration executed by this mode and uses the 64 KiB coalescing threshold across all query classes. Network p95 is the 20 ms / 300 Mbps profile with dependency rounds enforced.

| Experiment | Archive | vs GBZ | Index | Chunks | p50 / p95 bytes | p95 reads | p95 local | p95 network | Correct |
|---|---:|---:|---:|---:|---:|---:|---:|---:|:---:|
| fixed-w16k-zstd-3 | 5776885 B | 1.280x | 23942 B (0.41%); root 486 B / 6 pages | 304 | 83706 / 2060188 | 3 | 257553.9 us | 116.44 ms | yes |

## Query-size and hard-locus distributions

Results are not collapsed across size classes in `summary.json` or `queries.csv`. For the latency-first Pareto point at 64 KiB coalescing:

| Query group | Count | p50 / p95 / p99 bytes | p50 / p95 / p99 reads | p50 / p95 / p99 local | p95 20 ms network |
|---|---:|---:|---:|---:|---:|
| random-1000 | 10 | 31418 / 39112 / 39112 | 3 / 3 / 3 | 239.1 / 998.4 / 998.4 us | 62.54 ms |
| random-10000 | 10 | 42329 / 111073 / 111073 | 2 / 3 / 3 | 872.6 / 6300.7 / 6300.7 us | 64.46 ms |
| random-100000 | 10 | 131224 / 406284 / 406284 | 2 / 3 / 3 | 5233.6 / 52081.7 / 52081.7 us | 72.33 ms |
| random-1000000 | 10 | 1937040 / 2111837 / 2111837 | 3 / 3 / 3 | 243548.5 / 267150.5 / 267150.5 us | 117.82 ms |

## Two-level directory lookup

The selected archive has a 486-byte header/root bootstrap index and 6 leaf pages. Leaf bytes below count only bytes fetched beyond the 16 KiB bootstrap; a zero means the selected page was already resident in that prefix.

| Query group | p50 / p95 selected pages | p50 / p95 extra leaf bytes | p50 / p95 lookup |
|---|---:|---:|---:|
| random-1000 | 1 / 1 | 458 / 3011 | 5.4 / 15.0 us |
| random-10000 | 1 / 1 | 0 / 4089 | 5.7 / 31.5 us |
| random-100000 | 1 / 2 | 0 / 7100 | 8.1 / 31.6 us |
| random-1000000 | 2 / 3 | 7100 / 7100 | 15.9 / 21.9 us |

## Local latency comparison by query size

Local timings use the same deterministic queries. Candidate values use the 64 KiB coalescing threshold. The final column states the direction and factor relative to GBZ-base.

| Query group | GBZ p50 / p95 | GBZ-base p50 / p95 | Candidate p50 / p95 | Candidate vs GBZ-base p95 |
|---|---:|---:|---:|---:|
| random-1000 | 49.4 / 94.4 us | 184.8 / 463.0 us | 239.1 / 998.4 us | 2.16x slower |
| random-10000 | 709.4 / 5339.0 us | 2144.6 / 15379.9 us | 872.6 / 6300.7 us | 2.44x faster |
| random-100000 | 6455.7 / 85959.9 us | 18732.9 / 220744.3 us | 5233.6 / 52081.7 us | 4.24x faster |
| random-1000000 | 396993.5 / 446316.8 us | 973643.7 / 1077052.3 us | 243548.5 / 267150.5 us | 4.03x faster |

CPU breakdown for the same selected point (all query classes; microseconds):

| Component | p50 | p90 | p95 | p99 | max |
|---|---:|---:|---:|---:|---:|
| index lookup | 7.8 | 21.9 | 31.5 | 31.6 | 31.6 |
| decompression | 281.9 | 11391.2 | 11498.3 | 11805.8 | 11805.8 |
| binary decode | 291.5 | 33255.5 | 33762.1 | 34554.5 | 34554.5 |
| graph reconstruction | 793.3 | 164751.8 | 165818.7 | 172513.3 | 172513.3 |
| total local query | 1623.2 | 255924.9 | 257553.9 | 267150.5 | 267150.5 |

## Range coalescing

For `fixed-w16k-zstd-3`:

| Gap | p50 / p95 reads | p50 / p95 bytes | p95 20 ms network |
|---:|---:|---:|---:|
| 0 B | 3 / 3 | 83706 / 2060188 | 116.44 ms |
| 4096 B | 3 / 3 | 83706 / 2060188 | 116.44 ms |
| 16384 B | 3 / 3 | 83706 / 2060188 | 116.44 ms |
| 65536 B | 3 / 3 | 83706 / 2060188 | 116.44 ms |
| 262144 B | 3 / 3 | 83706 / 2060188 | 116.44 ms |
| 1048576 B | 3 / 3 | 83706 / 2060188 | 116.44 ms |

## Pareto frontier

The frontier jointly considers archive bytes, p95 reads, bytes, local time, and simulated 20 ms latency. In smoke mode it contains only the single measured candidate and is not a comparative winner.

| Experiment | Archive | p95 reads | p95 bytes | p95 local | p95 20 ms |
|---|---:|---:|---:|---:|---:|
| fixed-w16k-zstd-3 | 5776885 B | 3 | 2060188 | 257553.9 us | 116.44 ms |

## What we learned

- Fixed windows can answer every exercised locus from a small root bootstrap, an optional selected-leaf round, and one parallel data round while preserving exact local graph semantics.
- Query-size and named hard-locus groups remain separate in the retained distributions rather than being collapsed into one average.
- The latency-first Pareto point is `fixed-w16k-zstd-3`: 5776885 bytes (1.280x GBZ), with p95 3 reads, 2060188 bytes, and 116.44 ms under the 20 ms profile.
- GBZ-base remains storage-competitive relative to the materialized candidates at 2.283x GBZ, but its measured local p95 was 1072501.7 us and its synchronous SQLite access pattern is poorly matched to static-object range access.

## What failed or remains unresolved

- All requested interval sizes were exercised. The 10,000-query requirement remains deferred to a longer benchmark run.
- Archive expansion is input-specific: fixed headers, the root index, path metadata, and boundary duplication scale differently across fixtures.
- Single-config smoke mode cannot establish a new Pareto winner or attribute deduplication savings without a paired non-deduplicated build.
- Peak RSS is only available as whole-process `VmHWM` (Some(792904) KiB); per-phase construction/query RSS and CPU time are not inferred.
- This materialized representation does not preserve compressed GBWT records; a GBZ-record-preserving branch remains untested.

## What surprised us

Smoke mode isolates correctness and scale behavior of the 16 KiB/zstd-3 two-level-directory candidate; it does not establish a cross-layout winner.

## Next highest-information experiment

Repeat the 1 kb and 10 kb classes with at least 100 queries per size under controlled cache conditions, comparing 16 KiB and 64 KiB payloads. That separates the extra leaf-directory request cost from decode/reconstruction CPU before another full matrix.
