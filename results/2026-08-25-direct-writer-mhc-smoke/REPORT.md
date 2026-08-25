# Fixed-window cloud-layout experiment: 2026-08-25-direct-writer-mhc-smoke

This is a measured fixed-window archive v4 result for `test-data/mhc-10.gbz` in `single-config-smoke` mode, not a stable-format recommendation. Candidate rows independently passed query-graph comparison and exact weighted tile-local haplotype comparison.

## Baselines

| Baseline | Size | vs GBZ | Build/load | Query p50 / p95 | Storage behavior |
|---|---:|---:|---:|---:|---|
| GBZ | 4511832 B | 1.000x | load 18.024 ms | 3372.3 / 427294.0 us | whole graph must be loaded before interval extraction |
| GBZ-base | 10301440 B | 2.283x | build 448.758 ms; open 0.282 ms | 8484.6 / 1305796.9 us | local SQLite random access; not a static-object range layout |

SQLite page I/O was not observable in this run: strace child exited with exit status: 1

## Fixed-window candidates

The table contains every configuration executed by this mode and uses the 64 KiB coalescing threshold across all query classes. Network p95 is the 20 ms / 300 Mbps profile with dependency rounds enforced.

| Experiment | Archive | vs GBZ | Index | Chunks | p50 / p95 bytes | p95 reads | p95 local | p95 network | Correct |
|---|---:|---:|---:|---:|---:|---:|---:|---:|:---:|
| fixed-w16k-zstd-3 | 3194336 B | 0.708x | 41133 B (1.29%); root 173 B / 10 pages | 304 | 47685 / 941123 | 2 | 961344.6 us | 46.07 ms | yes |

## Query-size and hard-locus distributions

Results are not collapsed across size classes in `summary.json` or `queries.csv`. For the latency-first Pareto point at 64 KiB coalescing:

| Query group | Count | p50 / p95 / p99 bytes | p50 / p95 / p99 reads | p50 / p95 / p99 local | p95 20 ms network |
|---|---:|---:|---:|---:|---:|
| random-1000 | 10 | 11953 / 31964 / 31964 | 2 / 3 / 3 | 1836.8 / 6114.8 / 6114.8 us | 62.35 ms |
| random-10000 | 10 | 16833 / 45599 / 45599 | 1 / 2 / 2 | 5659.3 / 47035.4 / 47035.4 us | 42.22 ms |
| random-100000 | 10 | 72044 / 163627 / 163627 | 1 / 1 / 1 | 25810.3 / 224418.5 / 224418.5 us | 24.86 ms |
| random-1000000 | 10 | 898544 / 958875 / 958875 | 1 / 1 / 1 | 888221.7 / 971436.6 / 971436.6 us | 46.07 ms |

## Arithmetic manifest lookup

The selected archive has a 173-byte header/root bootstrap index and 10 fixed 4 KiB leaf pages. The root contains one manifest per reference path; a reader computes the required page offset directly from the query coordinate. Leaf bytes below count only bytes fetched beyond the 16 KiB bootstrap; a zero means the selected page was already resident in that prefix.

| Query group | p50 / p95 selected pages | p50 / p95 extra leaf bytes | p50 / p95 lookup |
|---|---:|---:|---:|
| random-1000 | 1 / 1 | 4096 / 4096 | 10.3 / 21.0 us |
| random-10000 | 1 / 2 | 0 / 4096 | 12.1 / 19.5 us |
| random-100000 | 1 / 2 | 0 / 0 | 27.4 / 47.7 us |
| random-1000000 | 3 / 3 | 0 / 0 | 72.9 / 127.7 us |

## Encoder construction

The v4 direct writer completed in 1670.384 ms with bounded directory metadata and raw/compressed queues. The first payload was written after 18.201 ms. It wrote a 0-byte payload spool, used 0 additional scratch bytes, and retained exactly 0 occurrence-index bytes. The temporary final archive held 41133 bytes before the first payload. Peak raw/compressed payload buffers and queues were 565982 / 51526 bytes; 0 adaptive splits were required.

| Phase | Wall time |
|---|---:|
| occurrence index (removed) | 0.000 ms |
| reference manifest discovery | 8.251 ms |
| topology preflight | 6.344 ms |
| local haplotype extraction | 614.499 ms |
| regional materialization | 867.394 ms |
| packed binary encoding | 16.977 ms |
| compression | 49.434 ms |
| writer finalization | 10.328 ms |
| archive validation | 71.057 ms |
| final copy (removed) | 0.000 ms |

The standalone release builds used the same archive bytes at `--threads 1` and
`--threads 4`, SHA-256
`119f8e15a0681bd4418ba0eba71c590ca2b6dfc79c1625243bde05fa1358d89a`.
One thread completed in 1754.494 ms; four threads completed in 1808.041 ms even
though compression fell from 54.149 ms to 26.521 ms. Ordered extraction remains
the bottleneck, so one thread stays the default.


All-vs-Distinct on the first tile `MHC-GRCh38#MHC` 0-16384: All emitted 9 traversals / 504 node visits / 47969 raw JSON bytes in 0.422 ms; Distinct emitted 6 traversals with total weight 9 / 504 weighted node visits / 40047 raw JSON bytes in 0.366 ms. Exact oriented-traversal aggregation matched: **true**.

Across all coalescing runs, 240 candidate query measurements passed both correctness gates and freshly checked 4338 tile payloads. The widest query selected 63 chunks.


## Local latency comparison by query size

Local timings use the same deterministic queries. Candidate values use the 64 KiB coalescing threshold. The final column states the direction and factor relative to GBZ-base.

| Query group | GBZ p50 / p95 | GBZ-base p50 / p95 | Candidate p50 / p95 | Candidate vs GBZ-base p95 |
|---|---:|---:|---:|---:|
| random-1000 | 35.2 / 111.2 us | 234.3 / 627.1 us | 1836.8 / 6114.8 us | 9.75x slower |
| random-10000 | 641.1 / 4968.3 us | 2753.3 / 19521.6 us | 5659.3 / 47035.4 us | 2.41x slower |
| random-100000 | 6298.1 / 78811.1 us | 23423.9 / 268367.1 us | 25810.3 / 224418.5 us | 1.20x faster |
| random-1000000 | 388973.0 / 427557.5 us | 1199404.0 / 1324942.0 us | 888221.7 / 971436.6 us | 1.36x faster |

CPU breakdown for the same selected point (all query classes; microseconds):

| Component | p50 | p90 | p95 | p99 | max |
|---|---:|---:|---:|---:|---:|
| index lookup | 18.3 | 77.5 | 110.4 | 127.7 | 127.7 |
| decompression | 243.3 | 5097.1 | 5364.1 | 5370.6 | 5370.6 |
| binary decode | 230.2 | 24781.9 | 25581.3 | 26078.5 | 26078.5 |
| graph reconstruction | 716.5 | 148727.0 | 153125.2 | 158277.1 | 158277.1 |
| total local query | 8788.5 | 922955.6 | 961344.6 | 971436.6 | 971436.6 |

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
| fixed-w16k-zstd-3 | 3194336 B | 2 | 941123 | 961344.6 us | 46.07 ms |

## What we learned

- Fixed windows can answer every exercised locus from a small root bootstrap, an optional selected-leaf round, and one parallel data round while preserving exact local graph semantics.
- Query-size and named hard-locus groups remain separate in the retained distributions rather than being collapsed into one average.
- The latency-first Pareto point is `fixed-w16k-zstd-3`: 3194336 bytes (0.708x GBZ), with p95 2 reads, 941123 bytes, and 46.07 ms under the 20 ms profile.
- GBZ-base remains storage-competitive relative to the materialized candidates at 2.283x GBZ, but its measured local p95 was 1305796.9 us and its synchronous SQLite access pattern is poorly matched to static-object range access.

## What failed or remains unresolved

- All requested interval sizes were exercised. The 10,000-query requirement remains deferred to a longer benchmark run.
- Archive expansion is input-specific: fixed headers, the root index, path metadata, and boundary duplication scale differently across fixtures.
- Single-config smoke mode cannot establish a new Pareto winner or attribute deduplication savings without a paired non-deduplicated build.
- Peak RSS is only available as whole-process `VmHWM` (Some(916932) KiB); per-phase construction/query RSS and CPU time are not inferred.
- This materialized representation does not preserve compressed GBWT records; a GBZ-record-preserving branch remains untested.

## What surprised us

Smoke mode isolates correctness and scale behavior of the 16 KiB/zstd-3 archive-v4 candidate; it does not establish a cross-layout winner.

## Next highest-information experiment

Prototype lazy or memory-mapped GBZ source access and repeat the bounded HPRC
chr6 pilot. Direct writing is now bounded, while full source deserialization and
path-index construction dominate time to first payload and process RSS.
