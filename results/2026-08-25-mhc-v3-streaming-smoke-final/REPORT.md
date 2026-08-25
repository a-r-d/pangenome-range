# Fixed-window cloud-layout experiment: 2026-08-25-mhc-v3-streaming-smoke-final

This is a measured fixed-window archive v3 result for `test-data/mhc-10.gbz` in `single-config-smoke` mode, not a stable-format recommendation. All candidate rows passed node, sequence, edge, path-multiplicity/orientation, and reference-coordinate canonical comparison.

## Baselines

| Baseline | Size | vs GBZ | Build/load | Query p50 / p95 | Storage behavior |
|---|---:|---:|---:|---:|---|
| GBZ | 4511832 B | 1.000x | load 21.503 ms | 7028.5 / 495121.1 us | whole graph must be loaded before interval extraction |
| GBZ-base | 10301440 B | 2.283x | build 522.439 ms; open 0.420 ms | 24603.9 / 1362296.7 us | local SQLite random access; not a static-object range layout |

SQLite page I/O was not observable in this run: strace child exited with exit status: 1

## Fixed-window candidates

The table contains every configuration executed by this mode and uses the 64 KiB coalescing threshold across all query classes. Network p95 is the 20 ms / 300 Mbps profile with dependency rounds enforced.

| Experiment | Archive | vs GBZ | Index | Chunks | p50 / p95 bytes | p95 reads | p95 local | p95 network | Correct |
|---|---:|---:|---:|---:|---:|---:|---:|---:|:---:|
| fixed-w16k-zstd-3 | 4020762 B | 0.891x | 41133 B (1.02%); root 173 B / 10 pages | 304 | 69223 / 1243809 | 3 | 272658.3 us | 74.17 ms | yes |

## Query-size and hard-locus distributions

Results are not collapsed across size classes in `summary.json` or `queries.csv`. For the latency-first Pareto point at 64 KiB coalescing:

| Query group | Count | p50 / p95 / p99 bytes | p50 / p95 / p99 reads | p50 / p95 / p99 local | p95 20 ms network |
|---|---:|---:|---:|---:|---:|
| random-1000 | 1 | 35662 / 35662 / 35662 | 3 / 3 / 3 | 653.5 / 653.5 / 653.5 us | 62.45 ms |
| random-10000 | 1 | 26101 / 26101 / 26101 | 2 / 2 / 2 | 824.9 / 824.9 / 824.9 us | 41.70 ms |
| random-100000 | 1 | 69223 / 69223 / 69223 | 1 / 1 / 1 | 5045.9 / 5045.9 / 5045.9 us | 22.35 ms |
| random-1000000 | 1 | 1243809 / 1243809 / 1243809 | 2 / 2 / 2 | 272658.3 / 272658.3 / 272658.3 us | 74.17 ms |

## Arithmetic manifest lookup

The selected archive has a 173-byte header/root bootstrap index and 10 fixed 4 KiB leaf pages. The root contains one manifest per reference path; a reader computes the required page offset directly from the query coordinate. Leaf bytes below count only bytes fetched beyond the 16 KiB bootstrap; a zero means the selected page was already resident in that prefix.

| Query group | p50 / p95 selected pages | p50 / p95 extra leaf bytes | p50 / p95 lookup |
|---|---:|---:|---:|
| random-1000 | 1 / 1 | 4096 / 4096 | 8.5 / 8.5 us |
| random-10000 | 1 / 1 | 4096 / 4096 | 21.2 / 21.2 us |
| random-100000 | 1 / 1 | 0 / 0 | 18.6 / 18.6 us |
| random-1000000 | 3 / 3 | 12288 / 12288 | 58.6 / 58.6 us |

## Encoder construction

The v3 encoder completed in 2762.420 ms while retaining only chunk metadata plus one regional raw/compressed payload at a time. It wrote a 3979629-byte payload spool and a temporary 41914368-byte disk-backed path-occurrence index. Peak raw/compressed payload buffers were 1126810 / 76741 bytes; 0 adaptive splits were required.

| Phase | Wall time |
|---|---:|
| temporary occurrence index | 843.069 ms |
| upstream regional selection | 366.885 ms |
| regional materialization | 1390.599 ms |
| packed binary encoding | 34.806 ms |
| compression | 63.440 ms |


## Local latency comparison by query size

Local timings use the same deterministic queries. Candidate values use the 64 KiB coalescing threshold. The final column states the direction and factor relative to GBZ-base.

| Query group | GBZ p50 / p95 | GBZ-base p50 / p95 | Candidate p50 / p95 | Candidate vs GBZ-base p95 |
|---|---:|---:|---:|---:|
| random-1000 | 111.8 / 111.8 us | 624.5 / 624.5 us | 653.5 / 653.5 us | 1.05x slower |
| random-10000 | 571.0 / 571.0 us | 2324.0 / 2324.0 us | 824.9 / 824.9 us | 2.82x faster |
| random-100000 | 7028.5 / 7028.5 us | 24603.9 / 24603.9 us | 5045.9 / 5045.9 us | 4.88x faster |
| random-1000000 | 495121.1 / 495121.1 us | 1362296.7 / 1362296.7 us | 272658.3 / 272658.3 us | 5.00x faster |

CPU breakdown for the same selected point (all query classes; microseconds):

| Component | p50 | p90 | p95 | p99 | max |
|---|---:|---:|---:|---:|---:|
| index lookup | 21.2 | 58.6 | 58.6 | 58.6 | 58.6 |
| decompression | 306.0 | 7466.3 | 7466.3 | 7466.3 | 7466.3 |
| binary decode | 681.7 | 32640.7 | 32640.7 | 32640.7 | 32640.7 |
| graph reconstruction | 3277.9 | 183505.5 | 183505.5 | 183505.5 | 183505.5 |
| total local query | 5045.9 | 272658.3 | 272658.3 | 272658.3 | 272658.3 |

## Range coalescing

For `fixed-w16k-zstd-3`:

| Gap | p50 / p95 reads | p50 / p95 bytes | p95 20 ms network |
|---:|---:|---:|---:|
| 0 B | 2 / 3 | 69223 / 1243809 | 74.17 ms |
| 4096 B | 2 / 3 | 69223 / 1243809 | 74.17 ms |
| 16384 B | 2 / 3 | 69223 / 1243809 | 74.17 ms |
| 65536 B | 2 / 3 | 69223 / 1243809 | 74.17 ms |
| 262144 B | 2 / 3 | 69223 / 1243809 | 74.17 ms |
| 1048576 B | 2 / 3 | 69223 / 1243809 | 74.17 ms |

## Pareto frontier

The frontier jointly considers archive bytes, p95 reads, bytes, local time, and simulated 20 ms latency. In smoke mode it contains only the single measured candidate and is not a comparative winner.

| Experiment | Archive | p95 reads | p95 bytes | p95 local | p95 20 ms |
|---|---:|---:|---:|---:|---:|
| fixed-w16k-zstd-3 | 4020762 B | 3 | 1243809 | 272658.3 us | 74.17 ms |

## What we learned

- Fixed windows can answer every exercised locus from a small root bootstrap, an optional selected-leaf round, and one parallel data round while preserving exact local graph semantics.
- Query-size and named hard-locus groups remain separate in the retained distributions rather than being collapsed into one average.
- The latency-first Pareto point is `fixed-w16k-zstd-3`: 4020762 bytes (0.891x GBZ), with p95 3 reads, 1243809 bytes, and 74.17 ms under the 20 ms profile.
- GBZ-base remains storage-competitive relative to the materialized candidates at 2.283x GBZ, but its measured local p95 was 1362296.7 us and its synchronous SQLite access pattern is poorly matched to static-object range access.

## What failed or remains unresolved

- All requested interval sizes were exercised. The 10,000-query requirement remains deferred to a longer benchmark run.
- Archive expansion is input-specific: fixed headers, the root index, path metadata, and boundary duplication scale differently across fixtures.
- Single-config smoke mode cannot establish a new Pareto winner or attribute deduplication savings without a paired non-deduplicated build.
- Peak RSS is only available as whole-process `VmHWM` (Some(352516) KiB); per-phase construction/query RSS and CPU time are not inferred.
- This materialized representation does not preserve compressed GBWT records; a GBZ-record-preserving branch remains untested.

## What surprised us

Smoke mode isolates correctness and scale behavior of the 16 KiB/zstd-3 archive-v3 candidate; it does not establish a cross-layout winner.

## Next highest-information experiment

Implement the TypeScript HTTP-range reader and rerun the 1 kb and 10 kb workloads in a real browser with explicit cold and warm cache phases. In parallel, generate the GB-scale fixture and use the retained encoder phase/RSS/disk metrics to validate linear scaling before attempting another full layout matrix.
