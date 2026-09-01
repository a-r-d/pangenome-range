# Fixed-window cloud-layout experiment: 2026-08-31-gbz-base-fixture-comparison

This is a measured fixed-window archive v1 result for `test-data/micb-kir3dl1.gbz` in `single-config-smoke` mode, not a stable-format recommendation. Candidate rows independently passed query-graph comparison and exact weighted tile-local haplotype comparison.

## Baselines

| Baseline | Size | vs GBZ | Build/load | Query p50 / p95 | Storage behavior |
|---|---:|---:|---:|---:|---|
| GBZ | 73920 B | 1.000x | load 0.353 ms | 7769.4 / 30158.4 us | whole graph must be loaded before interval extraction |
| GBZ-base | 172032 B | 2.327x | build 81.462 ms; open 0.264 ms | 14025.2 / 50511.8 us | local SQLite random access; not a static-object range layout |

SQLite page I/O was not observable in this run: strace child exited with exit status: 1

## Fixed-window candidates

The table contains every configuration executed by this mode and uses the 64 KiB coalescing threshold across all query classes. Network p95 is the 20 ms / 300 Mbps profile with dependency rounds enforced.

| Experiment | Archive | vs GBZ | Index | Chunks | p50 / p95 bytes | p95 reads | p95 local | p95 network | Correct |
|---|---:|---:|---:|---:|---:|---:|---:|---:|:---:|
| fixed-w16k-zstd-3 | 165052 B | 2.233x | 16944 B (10.27%); root 464 B / 4 pages | 8 | 42136 / 52327 | 1 | 104617.8 us | 21.91 ms | yes |

## Query-size and hard-locus distributions

Results are not collapsed across size classes in `summary.json` or `queries.csv`. For the latency-first Pareto point at 64 KiB coalescing:

| Query group | Count | p50 / p95 / p99 bytes | p50 / p95 / p99 reads | p50 / p95 / p99 local | p95 20 ms network |
|---|---:|---:|---:|---:|---:|
| hard-KIR3DL1 | 1 | 52327 / 52327 / 52327 | 2 / 2 / 2 | 105728.7 / 105728.7 / 105728.7 us | 42.40 ms |
| hard-MICB | 1 | 37797 / 37797 / 37797 | 2 / 2 / 2 | 35187.4 / 35187.4 / 35187.4 us | 42.01 ms |
| random-1000 | 50 | 21413 / 42764 / 52780 | 1 / 1 / 1 | 33060.2 / 95021.3 / 104250.2 us | 21.64 ms |
| random-10000 | 50 | 42136 / 52780 / 52780 | 1 / 1 / 1 | 98495.0 / 106157.2 / 108853.8 us | 21.91 ms |

## Arithmetic manifest lookup

The selected archive has a 464-byte header/root bootstrap index and 4 fixed 4 KiB leaf pages. The root contains one manifest per reference path; a reader computes the required page offset directly from the query coordinate. Leaf bytes below count only bytes fetched beyond the 16 KiB bootstrap; a zero means the selected page was already resident in that prefix.

| Query group | p50 / p95 selected pages | p50 / p95 extra leaf bytes | p50 / p95 lookup |
|---|---:|---:|---:|
| hard-KIR3DL1 | 1 / 1 | 560 / 560 | 13.3 / 13.3 us |
| hard-MICB | 1 / 1 | 0 / 0 | 7.7 / 7.7 us |
| random-1000 | 1 / 1 | 0 / 0 | 3.2 / 13.0 us |
| random-10000 | 1 / 1 | 0 / 0 | 3.3 / 4.1 us |

## Encoder construction

The v1 direct writer completed in 34.138 ms with bounded directory metadata and raw/compressed queues. The first payload was written after 8.305 ms. It wrote a 0-byte payload spool and exactly 0 occurrence-index bytes. Peak raw/compressed payload buffers were 201916 / 42764 bytes; 0 adaptive splits were required.

| Phase | Wall time |
|---|---:|
| occurrence index (removed) | 0.000 ms |
| reference manifest discovery | 0.002 ms |
| topology preflight | 0.000 ms |
| local haplotype extraction | 0.000 ms |
| regional materialization | 3.253 ms |
| packed binary encoding | 1.252 ms |
| compression | 2.052 ms |
| writer finalization | 2.888 ms |
| archive validation | 3.755 ms |
| final copy (removed) | 0.000 ms |

Across all coalescing runs, 612 candidate query measurements passed both correctness gates and freshly checked 894 tile payloads. The widest query selected 2 chunks.


## Local latency comparison by query size

Local timings use the same deterministic queries. Candidate values use the 64 KiB coalescing threshold. The final column states the direction and factor relative to GBZ-base.

| Query group | GBZ p50 / p95 | GBZ-base p50 / p95 | Candidate p50 / p95 | Candidate vs GBZ-base p95 |
|---|---:|---:|---:|---:|
| hard-KIR3DL1 | 42720.0 / 42720.0 us | 67930.3 / 67930.3 us | 105728.7 / 105728.7 us | 1.56x slower |
| hard-MICB | 11789.6 / 11789.6 us | 19605.0 / 19605.0 us | 35187.4 / 35187.4 us | 1.79x slower |
| random-1000 | 612.5 / 4495.1 us | 1829.4 / 10836.4 us | 33060.2 / 95021.3 us | 8.77x slower |
| random-10000 | 19683.2 / 31521.3 us | 33847.9 / 55727.7 us | 98495.0 / 106157.2 us | 1.90x slower |

CPU breakdown for the same selected point (all query classes; microseconds):

| Component | p50 | p90 | p95 | p99 | max |
|---|---:|---:|---:|---:|---:|
| index lookup | 3.2 | 4.3 | 8.1 | 14.5 | 16.7 |
| decompression | 158.0 | 206.7 | 210.5 | 217.7 | 247.2 |
| binary decode | 12706.6 | 16003.1 | 16226.4 | 16315.8 | 16389.3 |
| graph reconstruction | 1034.9 | 2709.0 | 3031.4 | 3359.5 | 3853.2 |
| total local query | 88592.9 | 104217.6 | 104617.8 | 106295.6 | 108853.8 |

## Range coalescing

For `fixed-w16k-zstd-3`:

| Gap | p50 / p95 reads | p50 / p95 bytes | p95 20 ms network |
|---:|---:|---:|---:|
| 0 B | 1 / 1 | 42136 / 52327 | 21.91 ms |
| 4096 B | 1 / 1 | 42136 / 52327 | 21.91 ms |
| 16384 B | 1 / 1 | 42136 / 52327 | 21.91 ms |
| 65536 B | 1 / 1 | 42136 / 52327 | 21.91 ms |
| 262144 B | 1 / 1 | 42136 / 52327 | 21.91 ms |
| 1048576 B | 1 / 1 | 42136 / 52327 | 21.91 ms |

## Pareto frontier

The frontier jointly considers archive bytes, p95 reads, bytes, local time, and simulated 20 ms latency. In smoke mode it contains only the single measured candidate and is not a comparative winner.

| Experiment | Archive | p95 reads | p95 bytes | p95 local | p95 20 ms |
|---|---:|---:|---:|---:|---:|
| fixed-w16k-zstd-3 | 165052 B | 1 | 52327 | 104617.8 us | 21.91 ms |

## What we learned

- Fixed windows can answer every exercised locus from a small root bootstrap, an optional selected-leaf round, and one parallel data round while preserving exact local graph semantics.
- Query-size and named hard-locus groups remain separate in the retained distributions rather than being collapsed into one average.
- The latency-first Pareto point is `fixed-w16k-zstd-3`: 165052 bytes (2.233x GBZ), with p95 1 reads, 52327 bytes, and 21.91 ms under the 20 ms profile.
- GBZ-base remains storage-competitive relative to the range archive at 2.327x GBZ, but its measured local p95 was 50511.8 us and its synchronous SQLite access pattern does not provide the archive's static-object HTTP range behavior.

## What failed or remains unresolved

- Query sizes [100000, 1000000] were skipped because no reference fragment in this fixture is long enough; the 10,000-query requirement is likewise deferred until chromosome scale.
- Archive expansion is input-specific: fixed headers, the root index, path metadata, and boundary duplication scale differently across fixtures.
- Single-config smoke mode cannot establish a new Pareto winner or attribute deduplication savings without a paired non-deduplicated build.
- Peak RSS is only available as whole-process `VmHWM` (Some(214032) KiB); per-phase construction/query RSS and CPU time are not inferred.
- Local query timings used uncontrolled OS page-cache state. They are not cold-storage, browser, or public-network measurements.

## What surprised us

Smoke mode isolates correctness and scale behavior of the 16 KiB/zstd-3 archive-v1 candidate; it does not establish a cross-layout winner.

## Next highest-information experiment

If this research resumes, repeat this exact current-v1 GBZ-base comparison on a chromosome-scale source. The fixture result is the cheap matched comparison; it is not a projection of chromosome-scale size or latency.
