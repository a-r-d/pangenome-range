# Fixed-window cloud-layout experiment: 2026-08-25-fixed-window-c0-final

This is a measured Candidate 0 result on the tiny MICB/KIR3DL1 fixture, not a format recommendation. All candidate rows passed node, sequence, edge, path-multiplicity/orientation, and reference-coordinate canonical comparison.

## Baselines

| Baseline | Size | vs GBZ | Build/load | Query p50 / p95 | Storage behavior |
|---|---:|---:|---:|---:|---|
| GBZ | 73920 B | 1.000x | load 0.383 ms | 8338.0 / 36767.3 us | whole graph must be loaded before interval extraction |
| GBZ-base | 172032 B | 2.327x | build 82.248 ms; open 0.306 ms | 14398.0 / 52363.1 us | local SQLite random access; not a static-object range layout |

A representative read-only GBZ-base MICB query issued 1,349 identifiable `pread64` calls and returned 99,188 bytes. The trace contained 19 page-sized reads, 1,329 repeated 16-byte SQLite header checks, and 1,344 non-sequential offset transitions. This was rerun outside the `ptrace`-restricted benchmark sandbox; it is process-level syscall evidence, not a cold-cache byte count.

## Fixed-window sweep

The table uses the 64 KiB coalescing threshold and all query classes. Network p95 is the 20 ms / 300 Mbps profile with dependency rounds enforced.

| Experiment | Archive | vs GBZ | Index | Chunks | p50 / p95 bytes | p95 reads | p95 local | p95 network | Correct |
|---|---:|---:|---:|---:|---:|---:|---:|---:|:---:|
| fixed-w16k-none | 5983700 B | 80.948x | 664 B (0.01%) | 8 | 773977 / 2217815 | 2 | 18348.4 us | 100.14 ms | yes |
| fixed-w16k-zstd-1 | 387828 B | 5.247x | 664 B (0.17%) | 8 | 88198 / 142571 | 2 | 19066.9 us | 44.80 ms | yes |
| fixed-w16k-zstd-3 | 309798 B | 4.191x | 664 B (0.21%) | 8 | 74148 / 113826 | 2 | 19272.5 us | 44.04 ms | yes |
| fixed-w16k-zstd-6 | 211756 B | 2.865x | 664 B (0.31%) | 8 | 57454 / 79470 | 2 | 19460.2 us | 43.12 ms | yes |
| fixed-w64k-none | 5932972 B | 80.262x | 442 B (0.01%) | 5 | 1798619 / 2201592 | 2 | 13920.2 us | 99.71 ms | yes |
| fixed-w64k-zstd-1 | 456866 B | 6.181x | 442 B (0.10%) | 5 | 109426 / 211471 | 2 | 15001.1 us | 46.64 ms | yes |
| fixed-w64k-zstd-3 | 293348 B | 3.968x | 442 B (0.15%) | 5 | 92283 / 102416 | 2 | 17338.6 us | 43.73 ms | yes |
| fixed-w64k-zstd-6 | 206052 B | 2.788x | 442 B (0.21%) | 5 | 68024 / 76605 | 2 | 14659.2 us | 43.04 ms | yes |
| fixed-w256k-none | 5854944 B | 79.206x | 368 B (0.01%) | 4 | 2201592 / 2201592 | 2 | 14518.1 us | 99.71 ms | yes |
| fixed-w256k-zstd-1 | 535442 B | 7.244x | 368 B (0.07%) | 4 | 211471 / 211471 | 2 | 15157.1 us | 46.64 ms | yes |
| fixed-w256k-zstd-3 | 283334 B | 3.833x | 368 B (0.13%) | 4 | 102416 / 102416 | 2 | 15126.8 us | 43.73 ms | yes |
| fixed-w256k-zstd-6 | 200492 B | 2.712x | 368 B (0.18%) | 4 | 76605 / 76605 | 2 | 14864.8 us | 43.04 ms | yes |
| fixed-w1m-none | 5854944 B | 79.206x | 368 B (0.01%) | 4 | 2201592 / 2201592 | 2 | 14924.0 us | 99.71 ms | yes |
| fixed-w1m-zstd-1 | 535442 B | 7.244x | 368 B (0.07%) | 4 | 211471 / 211471 | 2 | 16336.8 us | 46.64 ms | yes |
| fixed-w1m-zstd-3 | 283334 B | 3.833x | 368 B (0.13%) | 4 | 102416 / 102416 | 2 | 16598.5 us | 43.73 ms | yes |
| fixed-w1m-zstd-6 | 200492 B | 2.712x | 368 B (0.18%) | 4 | 76605 / 76605 | 2 | 16016.8 us | 43.04 ms | yes |
| fixed-w4m-none | 5854944 B | 79.206x | 368 B (0.01%) | 4 | 2201592 / 2201592 | 2 | 15533.7 us | 99.71 ms | yes |
| fixed-w4m-zstd-1 | 535442 B | 7.244x | 368 B (0.07%) | 4 | 211471 / 211471 | 2 | 16483.1 us | 46.64 ms | yes |
| fixed-w4m-zstd-3 | 283334 B | 3.833x | 368 B (0.13%) | 4 | 102416 / 102416 | 2 | 16092.1 us | 43.73 ms | yes |
| fixed-w4m-zstd-6 | 200492 B | 2.712x | 368 B (0.18%) | 4 | 76605 / 76605 | 2 | 15269.5 us | 43.04 ms | yes |

## Query-size and hard-locus distributions

Results are not collapsed across size classes in `summary.json` or `queries.csv`. For the latency-first Pareto point at 64 KiB coalescing:

| Query group | Count | p50 / p95 / p99 bytes | p50 / p95 / p99 reads | p50 / p95 / p99 local | p95 20 ms network |
|---|---:|---:|---:|---:|---:|
| hard-KIR3DL1 | 1 | 76605 / 76605 / 76605 | 2 / 2 / 2 | 15584.5 / 15584.5 / 15584.5 us | 43.04 ms |
| hard-MICB | 1 | 40209 / 40209 / 40209 | 2 / 2 / 2 | 4870.3 / 4870.3 / 4870.3 us | 42.07 ms |
| random-1000 | 100 | 76605 / 76605 / 76605 | 2 / 2 / 2 | 10783.1 / 11651.4 / 11883.1 us | 43.04 ms |
| random-10000 | 100 | 40209 / 76605 / 76605 | 2 / 2 / 2 | 5067.0 / 14845.5 / 15700.9 us | 43.04 ms |

CPU breakdown for the same selected point (all query classes; microseconds):

| Component | p50 | p90 | p95 | p99 | max |
|---|---:|---:|---:|---:|---:|
| index lookup | 0.6 | 9.4 | 10.6 | 134.9 | 170.8 |
| decompression | 430.7 | 453.6 | 467.8 | 490.9 | 493.5 |
| binary decode | 6018.4 | 6333.0 | 6391.6 | 6535.2 | 6540.1 |
| graph reconstruction | 3346.1 | 6761.0 | 7074.1 | 7722.4 | 8172.4 |
| total local query | 10660.6 | 14367.8 | 14661.8 | 15137.4 | 15700.9 |

## Range coalescing

For the improvement baseline `fixed-w256k-zstd-6`:

| Gap | p50 / p95 reads | p50 / p95 bytes | p95 20 ms network |
|---:|---:|---:|---:|
| 0 B | 2 / 2 | 76605 / 76605 | 43.04 ms |
| 4096 B | 2 / 2 | 76605 / 76605 | 43.04 ms |
| 16384 B | 2 / 2 | 76605 / 76605 | 43.04 ms |
| 65536 B | 2 / 2 | 76605 / 76605 | 43.04 ms |
| 262144 B | 2 / 2 | 76605 / 76605 | 43.04 ms |
| 1048576 B | 2 / 2 | 76605 / 76605 | 43.04 ms |

## Exactly one measured structural improvement

The baseline sweep exposed 2 exact repeated chunk payload entries accounted for 100062 avoidable compressed bytes (49.9% of the measured baseline archive). I therefore implemented only exact content-addressed chunk deduplication: directory entries may share one independently decodable physical payload when their uncompressed bytes match exactly. No fuzzy or cross-chunk dictionary dependency was introduced.

| Metric | Before | After |
|---|---:|---:|
| Experiment | `fixed-w256k-zstd-6` | `fixed-w256k-zstd-6-dedup` |
| Archive bytes | 200492 | 100430 |
| Bytes saved | 0 | 100062 |
| Physical chunks removed | 0 | 2 |
| p95 bytes fetched | 76605 | 76605 |
| p95 physical reads | 2 | 2 |
| p95 20 ms network | 43.04 ms | 43.04 ms |
| Correctness | pass | pass |

## Pareto frontier

The frontier jointly considers archive bytes, p95 reads, bytes, local time, and simulated 20 ms latency. Timing noise can leave near-equivalent points on this tiny fixture.

| Experiment | Archive | p95 reads | p95 bytes | p95 local | p95 20 ms |
|---|---:|---:|---:|---:|---:|
| fixed-w256k-zstd-6-dedup | 100430 B | 2 | 76605 | 14661.8 us | 43.04 ms |
| fixed-w64k-zstd-6 | 206052 B | 2 | 76605 | 14659.2 us | 43.04 ms |
| fixed-w256k-none | 5854944 B | 2 | 2201592 | 14518.1 us | 99.71 ms |
| fixed-w64k-none | 5932972 B | 2 | 2201592 | 13920.2 us | 99.71 ms |

## What we learned

- Fixed windows can answer every exercised locus from a small bootstrap plus one parallel data round while preserving exact local graph semantics.
- Coordinate span is a poor proxy for graph payload even in two loci; MICB and KIR3DL1 remain separately visible in the retained distributions.
- The latency-first Pareto point is `fixed-w256k-zstd-6-dedup`: 100430 bytes (1.359x GBZ), with p95 2 reads, 76605 bytes, and 43.04 ms under the 20 ms profile.
- GBZ-base remains storage-competitive relative to the materialized candidates at 2.327x GBZ, but its measured local p95 was 52363.1 us and its synchronous SQLite access pattern is poorly matched to static-object range access.

## What failed or remains unresolved

- Query sizes [100000, 1000000] were skipped because no reference fragment in this fixture is long enough; the 10,000-query requirement is likewise deferred until chromosome scale.
- Archive expansion on this tiny fixture is not representative: fixed headers, the root index, and path metadata are a large fraction of 73,920 source bytes.
- The best pre-improvement materialized archive still missed the 1.50x prototype envelope; its measured expansion was 2.712x GBZ.
- Peak RSS is only available as whole-process `VmHWM` (Some(533896) KiB); per-phase construction/query RSS and CPU time are not inferred from this small run.
- This materialized representation does not preserve compressed GBWT records; a GBZ-record-preserving branch remains untested.

## What surprised us

Exact regional payloads recurred across distinct reference directory entries often enough to save 100062 bytes without changing query semantics or introducing a decode dependency. Coalescing beyond adjacency had limited value because path-local coordinate ordering already made each query's required payloads contiguous.

## Next highest-information experiment

Run the same retained matrix on one HPRC chromosome, adding a GBZ-record-preserving representation beside this locally materialized encoding. That scale will reveal whether path metadata/halo duplication or decompressed regional materialization is the dominant expansion source, and it enables the required 100 kb, 1 Mb, and 10,000-query workloads.
