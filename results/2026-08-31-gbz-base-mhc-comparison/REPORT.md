# Fixed-window cloud-layout experiment: 2026-08-31-gbz-base-mhc-comparison

This is a measured fixed-window archive v1 result for `test-data/mhc-10.gbz` in `single-config-smoke` mode, not a stable-format recommendation. Candidate rows independently passed query-graph comparison and exact weighted tile-local haplotype comparison.

## Baselines

| Baseline | Size | vs GBZ | Build/load | Query p50 / p95 | Storage behavior |
|---|---:|---:|---:|---:|---|
| GBZ | 4511832 B | 1.000x | load 18.623 ms | 2776.9 / 410687.5 us | whole graph must be loaded before interval extraction |
| GBZ-base | 10301440 B | 2.283x | build 463.130 ms; open 0.313 ms | 9491.9 / 1248997.9 us | local SQLite random access; not a static-object range layout |

SQLite page I/O was not observable in this run: strace child exited with exit status: 1

## Fixed-window candidates

The table contains every configuration executed by this mode and uses the 64 KiB coalescing threshold across all query classes. Network p95 is the 20 ms / 300 Mbps profile with dependency rounds enforced.

| Experiment | Archive | vs GBZ | Index | Chunks | p50 / p95 bytes | p95 reads | p95 local | p95 network | Correct |
|---|---:|---:|---:|---:|---:|---:|---:|---:|:---:|
| fixed-w16k-zstd-3 | 4807424 B | 1.066x | 41229 B (0.86%); root 173 B / 10 pages | 304 | 56292 / 1672300 | 1 | 1069584.8 us | 65.09 ms | yes |

## Query-size and hard-locus distributions

Results are not collapsed across size classes in `summary.json` or `queries.csv`. For the latency-first Pareto point at 64 KiB coalescing:

| Query group | Count | p50 / p95 / p99 bytes | p50 / p95 / p99 reads | p50 / p95 / p99 local | p95 20 ms network |
|---|---:|---:|---:|---:|---:|
| random-1000 | 50 | 10751 / 33881 / 44302 | 1 / 2 / 3 | 2463.9 / 16048.2 / 26527.5 us | 41.52 ms |
| random-10000 | 50 | 19328 / 89591 / 199777 | 1 / 1 / 1 | 6094.1 / 49018.5 / 137788.1 us | 22.89 ms |
| random-100000 | 50 | 78204 / 413945 / 600114 | 1 / 1 / 1 | 22003.7 / 295081.4 / 461389.5 us | 31.54 ms |
| random-1000000 | 50 | 878108 / 1762610 / 1770071 | 1 / 1 / 1 | 383026.5 / 1171802.6 / 1250590.2 us | 67.50 ms |

## Arithmetic manifest lookup

The selected archive has a 173-byte header/root bootstrap index and 10 fixed 4 KiB leaf pages. The root contains one manifest per reference path; a reader computes the required page offset directly from the query coordinate. Leaf bytes below count only bytes fetched beyond the 16 KiB bootstrap; a zero means the selected page was already resident in that prefix.

| Query group | p50 / p95 selected pages | p50 / p95 extra leaf bytes | p50 / p95 lookup |
|---|---:|---:|---:|
| random-1000 | 1 / 1 | 0 / 4096 | 7.7 / 26.0 us |
| random-10000 | 1 / 1 | 0 / 0 | 16.8 / 52.7 us |
| random-100000 | 1 / 2 | 0 / 0 | 38.8 / 94.8 us |
| random-1000000 | 3 / 3 | 0 / 0 | 149.8 / 219.1 us |

## Encoder construction

The v1 direct writer completed in 583.022 ms with bounded directory metadata and raw/compressed queues. The first payload was written after 7.100 ms. It wrote a 0-byte payload spool and exactly 0 occurrence-index bytes. Peak raw/compressed payload buffers were 727680 / 139586 bytes; 0 adaptive splits were required.

| Phase | Wall time |
|---|---:|
| occurrence index (removed) | 0.000 ms |
| reference manifest discovery | 0.005 ms |
| topology preflight | 0.000 ms |
| local haplotype extraction | 0.000 ms |
| regional materialization | 88.244 ms |
| packed binary encoding | 32.832 ms |
| compression | 73.117 ms |
| writer finalization | 10.959 ms |
| archive validation | 86.350 ms |
| final copy (removed) | 0.000 ms |

Across all coalescing runs, 1200 candidate query measurements passed both correctness gates and freshly checked 21546 tile payloads. The widest query selected 62 chunks.


## Local latency comparison by query size

Local timings use the same deterministic queries. Candidate values use the 64 KiB coalescing threshold. The final column states the direction and factor relative to GBZ-base.

| Query group | GBZ p50 / p95 | GBZ-base p50 / p95 | Candidate p50 / p95 | Candidate vs GBZ-base p95 |
|---|---:|---:|---:|---:|
| random-1000 | 48.3 / 454.5 us | 290.0 / 2935.7 us | 2463.9 / 16048.2 us | 5.47x slower |
| random-10000 | 654.6 / 3777.8 us | 2812.7 / 14494.5 us | 6094.1 / 49018.5 us | 3.38x slower |
| random-100000 | 6122.1 / 86868.9 us | 22854.4 / 287232.4 us | 22003.7 / 295081.4 us | 1.03x slower |
| random-1000000 | 153751.7 / 445742.1 us | 466825.7 / 1336845.3 us | 383026.5 / 1171802.6 us | 1.14x faster |

CPU breakdown for the same selected point (all query classes; microseconds):

| Component | p50 | p90 | p95 | p99 | max |
|---|---:|---:|---:|---:|---:|
| index lookup | 29.2 | 155.6 | 182.5 | 219.6 | 225.2 |
| decompression | 257.6 | 4773.5 | 7375.1 | 7976.0 | 7990.8 |
| binary decode | 1530.9 | 54725.2 | 106751.9 | 121921.7 | 129219.7 |
| graph reconstruction | 1005.3 | 73246.2 | 151760.5 | 164798.1 | 165852.6 |
| total local query | 14053.6 | 543288.0 | 1069584.8 | 1198510.2 | 1250590.2 |

## Range coalescing

For `fixed-w16k-zstd-3`:

| Gap | p50 / p95 reads | p50 / p95 bytes | p95 20 ms network |
|---:|---:|---:|---:|
| 0 B | 1 / 1 | 56292 / 1672300 | 65.09 ms |
| 4096 B | 1 / 1 | 56292 / 1672300 | 65.09 ms |
| 16384 B | 1 / 1 | 56292 / 1672300 | 65.09 ms |
| 65536 B | 1 / 1 | 56292 / 1672300 | 65.09 ms |
| 262144 B | 1 / 1 | 56292 / 1672300 | 65.09 ms |
| 1048576 B | 1 / 1 | 56292 / 1672300 | 65.09 ms |

## Pareto frontier

The frontier jointly considers archive bytes, p95 reads, bytes, local time, and simulated 20 ms latency. In smoke mode it contains only the single measured candidate and is not a comparative winner.

| Experiment | Archive | p95 reads | p95 bytes | p95 local | p95 20 ms |
|---|---:|---:|---:|---:|---:|
| fixed-w16k-zstd-3 | 4807424 B | 1 | 1672300 | 1069584.8 us | 65.09 ms |

## What we learned

- Fixed windows can answer every exercised locus from a small root bootstrap, an optional selected-leaf round, and one parallel data round while preserving exact local graph semantics.
- Query-size and named hard-locus groups remain separate in the retained distributions rather than being collapsed into one average.
- The latency-first Pareto point is `fixed-w16k-zstd-3`: 4807424 bytes (1.066x GBZ), with p95 1 reads, 1672300 bytes, and 65.09 ms under the 20 ms profile.
- GBZ-base remains storage-competitive relative to the range archive at 2.283x GBZ, but its measured local p95 was 1248997.9 us and its synchronous SQLite access pattern does not provide the archive's static-object HTTP range behavior.

## What failed or remains unresolved

- All requested interval sizes were exercised. The 10,000-query requirement remains deferred to a longer benchmark run.
- Archive expansion is input-specific: fixed headers, the root index, path metadata, and boundary duplication scale differently across fixtures.
- Single-config smoke mode cannot establish a new Pareto winner or attribute deduplication savings without a paired non-deduplicated build.
- Peak RSS is only available as whole-process `VmHWM` (Some(1927456) KiB); per-phase construction/query RSS and CPU time are not inferred.
- Local query timings used uncontrolled OS page-cache state. They are not cold-storage, browser, or public-network measurements.

## What surprised us

Smoke mode isolates correctness and scale behavior of the 16 KiB/zstd-3 archive-v1 candidate; it does not establish a cross-layout winner.

## Next highest-information experiment

If this research resumes, repeat this exact current-v1 GBZ-base comparison on a chromosome-scale source. The fixture result is the cheap matched comparison; it is not a projection of chromosome-scale size or latency.
