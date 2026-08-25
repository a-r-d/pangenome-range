# Fixed-window cloud-layout experiment: 2026-08-25-mhc-fixed-window-c0-full

This is a measured Candidate 0 result for `test-data/mhc-10.gbz` in `full-sweep` mode, not a format recommendation. All candidate rows passed node, sequence, edge, path-multiplicity/orientation, and reference-coordinate canonical comparison.

## Baselines

| Baseline | Size | vs GBZ | Build/load | Query p50 / p95 | Storage behavior |
|---|---:|---:|---:|---:|---|
| GBZ | 4511832 B | 1.000x | load 18.925 ms | 2507.2 / 458617.0 us | whole graph must be loaded before interval extraction |
| GBZ-base | 10301440 B | 2.283x | build 472.482 ms; open 0.390 ms | 6835.5 / 1100565.8 us | local SQLite random access; not a static-object range layout |

A representative GBZ-base query issued 92 identifiable `pread64` calls and returned 50516 bytes. The trace contained 12 page-sized reads, 79 repeated 16-byte SQLite header checks, and 91 non-sequential offset transitions. This is process-level syscall evidence, not a cold-cache byte count.

## Fixed-window candidates

The table contains every configuration executed by this mode and uses the 64 KiB coalescing threshold across all query classes. Network p95 is the 20 ms / 300 Mbps profile with dependency rounds enforced.

| Experiment | Archive | vs GBZ | Index | Chunks | p50 / p95 bytes | p95 reads | p95 local | p95 network | Correct |
|---|---:|---:|---:|---:|---:|---:|---:|---:|:---:|
| fixed-w16k-none | 32499369 B | 7.203x | 23480 B (0.07%) | 304 | 279889 / 14118805 | 3 | 259106.8 us | 438.00 ms | yes |
| fixed-w16k-zstd-1 | 6755220 B | 1.497x | 23480 B (0.35%) | 304 | 91894 / 2655900 | 3 | 274367.8 us | 132.32 ms | yes |
| fixed-w16k-zstd-3 | 5776423 B | 1.280x | 23480 B (0.41%) | 304 | 90802 / 2060184 | 3 | 271662.9 us | 116.44 ms | yes |
| fixed-w16k-zstd-6 | 5603443 B | 1.242x | 23480 B (0.42%) | 304 | 87030 / 2084726 | 3 | 287214.7 us | 117.09 ms | yes |
| fixed-w64k-none | 31795345 B | 7.047x | 5924 B (0.02%) | 76 | 493687 / 14251068 | 2 | 278541.4 us | 421.03 ms | yes |
| fixed-w64k-zstd-1 | 6232278 B | 1.381x | 5924 B (0.10%) | 76 | 111426 / 2528081 | 2 | 283373.3 us | 108.42 ms | yes |
| fixed-w64k-zstd-3 | 4856221 B | 1.076x | 5924 B (0.12%) | 76 | 99050 / 1760716 | 2 | 304668.9 us | 87.95 ms | yes |
| fixed-w64k-zstd-6 | 4896771 B | 1.085x | 5924 B (0.12%) | 76 | 91893 / 1879087 | 2 | 273015.8 us | 91.11 ms | yes |
| fixed-w256k-none | 31640563 B | 7.013x | 1535 B (0.00%) | 19 | 1563403 / 14750811 | 2 | 273883.7 us | 434.35 ms | yes |
| fixed-w256k-zstd-1 | 5616059 B | 1.245x | 1535 B (0.03%) | 19 | 322122 / 2420103 | 2 | 324013.5 us | 105.54 ms | yes |
| fixed-w256k-zstd-3 | 4273073 B | 0.947x | 1535 B (0.04%) | 19 | 241598 / 1700198 | 2 | 308578.3 us | 86.34 ms | yes |
| fixed-w256k-zstd-6 | 4547204 B | 1.008x | 1535 B (0.03%) | 19 | 256647 / 1910504 | 2 | 286724.3 us | 91.95 ms | yes |
| fixed-w1m-none | 31610495 B | 7.006x | 457 B (0.00%) | 5 | 7627919 / 19657770 | 2 | 277738.6 us | 565.21 ms | yes |
| fixed-w1m-zstd-1 | 5245981 B | 1.163x | 457 B (0.01%) | 5 | 1235717 / 3008347 | 2 | 282208.2 us | 121.22 ms | yes |
| fixed-w1m-zstd-3 | 4158651 B | 0.922x | 457 B (0.01%) | 5 | 999997 / 2362785 | 2 | 294114.6 us | 104.01 ms | yes |
| fixed-w1m-zstd-6 | 4505033 B | 0.998x | 457 B (0.01%) | 5 | 1102160 / 2660700 | 2 | 274059.1 us | 111.95 ms | yes |
| fixed-w4m-none | 31604483 B | 7.005x | 226 B (0.00%) | 2 | 27824886 / 31604483 | 2 | 394139.8 us | 883.79 ms | yes |
| fixed-w4m-zstd-1 | 5158087 B | 1.143x | 226 B (0.00%) | 2 | 4452200 / 5158087 | 2 | 400398.2 us | 178.55 ms | yes |
| fixed-w4m-zstd-3 | 4466559 B | 0.990x | 226 B (0.01%) | 2 | 3949664 / 4466559 | 2 | 403464.2 us | 160.11 ms | yes |
| fixed-w4m-zstd-6 | 4931344 B | 1.093x | 226 B (0.00%) | 2 | 4378505 / 4931344 | 2 | 389487.4 us | 172.50 ms | yes |

## Query-size and hard-locus distributions

Results are not collapsed across size classes in `summary.json` or `queries.csv`. For the latency-first Pareto point at 64 KiB coalescing:

| Query group | Count | p50 / p95 / p99 bytes | p50 / p95 / p99 reads | p50 / p95 / p99 local | p95 20 ms network |
|---|---:|---:|---:|---:|---:|
| random-1000 | 10 | 170216 / 430171 / 430171 | 2 / 2 / 2 | 5724.9 / 36776.8 / 36776.8 us | 52.47 ms |
| random-10000 | 10 | 228539 / 1157007 / 1157007 | 2 / 2 / 2 | 11411.1 / 108980.5 / 108980.5 us | 71.85 ms |
| random-100000 | 10 | 241598 / 1157007 / 1157007 | 2 / 2 / 2 | 13844.9 / 134495.9 / 134495.9 us | 71.85 ms |
| random-1000000 | 10 | 1616395 / 1700198 / 1700198 | 2 / 2 / 2 | 260788.7 / 309150.8 / 309150.8 us | 86.34 ms |

## Local latency comparison by query size

Local timings use the same deterministic queries. Candidate values use the 64 KiB coalescing threshold. The final column states the direction and factor relative to GBZ-base.

| Query group | GBZ p50 / p95 | GBZ-base p50 / p95 | Candidate p50 / p95 | Candidate vs GBZ-base p95 |
|---|---:|---:|---:|---:|
| random-1000 | 47.3 / 98.4 us | 177.8 / 494.6 us | 5724.9 / 36776.8 us | 74.35x slower |
| random-10000 | 720.8 / 5418.3 us | 2131.8 / 15755.5 us | 11411.1 / 108980.5 us | 6.92x slower |
| random-100000 | 6660.9 / 92205.2 us | 19703.0 / 231434.0 us | 13844.9 / 134495.9 us | 1.72x faster |
| random-1000000 | 421574.2 / 468414.6 us | 1031768.8 / 1132578.1 us | 260788.7 / 309150.8 us | 3.66x faster |

CPU breakdown for the same selected point (all query classes; microseconds):

| Component | p50 | p90 | p95 | p99 | max |
|---|---:|---:|---:|---:|---:|
| index lookup | 0.9 | 2.1 | 8.0 | 10.9 | 10.9 |
| decompression | 1348.0 | 11856.0 | 12159.0 | 12500.2 | 12500.2 |
| binary decode | 4112.8 | 42536.7 | 43636.3 | 44024.9 | 44024.9 |
| graph reconstruction | 5472.7 | 182530.2 | 199213.8 | 200713.4 | 200713.4 |
| total local query | 13844.3 | 290554.5 | 308578.3 | 309150.8 | 309150.8 |

## Range coalescing

For `fixed-w256k-zstd-3`:

| Gap | p50 / p95 reads | p50 / p95 bytes | p95 20 ms network |
|---:|---:|---:|---:|
| 0 B | 2 / 2 | 241598 / 1700198 | 86.34 ms |
| 4096 B | 2 / 2 | 241598 / 1700198 | 86.34 ms |
| 16384 B | 2 / 2 | 241598 / 1700198 | 86.34 ms |
| 65536 B | 2 / 2 | 241598 / 1700198 | 86.34 ms |
| 262144 B | 2 / 2 | 241598 / 1700198 | 86.34 ms |
| 1048576 B | 2 / 2 | 241598 / 1700198 | 86.34 ms |

## Pareto frontier

The frontier jointly considers archive bytes, p95 reads, bytes, local time, and simulated 20 ms latency. Timing noise can leave near-equivalent points on the frontier.

| Experiment | Archive | p95 reads | p95 bytes | p95 local | p95 20 ms |
|---|---:|---:|---:|---:|---:|
| fixed-w1m-zstd-3 | 4158651 B | 2 | 2362785 | 294114.6 us | 104.01 ms |
| fixed-w256k-zstd-3 | 4273073 B | 2 | 1700198 | 308578.3 us | 86.34 ms |
| fixed-w1m-zstd-6 | 4505033 B | 2 | 2660700 | 274059.1 us | 111.95 ms |
| fixed-w256k-zstd-6 | 4547204 B | 2 | 1910504 | 286724.3 us | 91.95 ms |
| fixed-w64k-zstd-3 | 4856221 B | 2 | 1760716 | 304668.9 us | 87.95 ms |
| fixed-w64k-zstd-6 | 4896771 B | 2 | 1879087 | 273015.8 us | 91.11 ms |
| fixed-w16k-zstd-3 | 5776423 B | 3 | 2060184 | 271662.9 us | 116.44 ms |
| fixed-w16k-none | 32499369 B | 3 | 14118805 | 259106.8 us | 438.00 ms |

## What we learned

- Fixed windows can answer every exercised locus from a small bootstrap plus one parallel data round while preserving exact local graph semantics.
- Query-size and named hard-locus groups remain separate in the retained distributions rather than being collapsed into one average.
- The latency-first Pareto point is `fixed-w256k-zstd-3`: 4273073 bytes (0.947x GBZ), with p95 2 reads, 1700198 bytes, and 86.34 ms under the 20 ms profile.
- GBZ-base remains storage-competitive relative to the materialized candidates at 2.283x GBZ, but its measured local p95 was 1100565.8 us and its synchronous SQLite access pattern is poorly matched to static-object range access.

## What failed or remains unresolved

- All requested interval sizes were exercised. The 10,000-query requirement remains deferred to a longer benchmark run.
- Archive expansion is input-specific: fixed headers, the root index, path metadata, and boundary duplication scale differently across fixtures.
- The full sweep found no exact duplicate regional payloads, so it correctly skipped an unjustified deduplication follow-up.
- Peak RSS is only available as whole-process `VmHWM` (Some(792892) KiB); per-phase construction/query RSS and CPU time are not inferred.
- This materialized representation does not preserve compressed GBWT records; a GBZ-record-preserving branch remains untested.

## What surprised us

Unlike the tiny fixture, this input produced no exact duplicate regional payloads in the measured matrix. Deduplication is therefore not assumed to help at this scale.

## Next highest-information experiment

Run the same retained matrix on one HPRC chromosome, adding a GBZ-record-preserving representation beside this locally materialized encoding. That scale will reveal whether path metadata/halo duplication or decompressed regional materialization is the dominant expansion source, and it enables the required 100 kb, 1 Mb, and 10,000-query workloads.
