# Benchmark definitions

Define the workload and metrics before comparing layouts. All timed benchmarks
must use release builds and record the exact source object checksum, converter
commit, dependency versions, host, storage medium, and network setup.

## Query workload

A query case records reference sample, contig, half-open interval, requested
context policy, path/haplotype output policy, and oracle canonical hash. Corpora
should include fixed biological loci (MICB and KIR3DL1), stratified interval
sizes, random reproducible intervals, graph-dense/repetitive regions, boundaries,
and negative/absent-coordinate cases.

## Storage metrics

- **Source bytes:** byte length of the exact GBZ input.
- **Archive bytes:** all bytes required to serve a candidate, including indexes
  and sidecars.
- **Size expansion:** `archive bytes / source bytes`.
- **Construction wall time / peak memory:** measured separately from queries.

## Range metrics

- **Read operation:** one `read_range` call, successful or not.
- **Total bytes requested:** sum of requested lengths.
- **Unique bytes requested:** union length of all half-open requested intervals.
- **Duplicate bytes requested:** total minus unique bytes.
- **Payload bytes used:** bytes actually consumed by parsing/decompression, to be
  instrumented per candidate.
- **Read amplification:** unique requested bytes divided by the minimum encoded
  bytes needed for the returned semantic payload. Until that denominator is
  measurable, report both terms rather than inventing a ratio.
- **Mergeable read:** a nonempty request overlapping or adjacent to another
  request after sorting by offset.
- **Request dependency depth:** serial read rounds required by lookup logic. This
  is distinct from request count. Candidate experiments explicitly model the
  bootstrap/index dependency groups followed by the data group.

Always retain the raw range list; aggregates alone cannot reveal scatter or a
serial index walk.

## Timing metrics

- **Warm/cold local query time:** wall time with cache state explicitly stated.
- **Time to first required metadata:** real transport timing, not simulated.
- **End-to-end query time:** request start through verified canonical result.
- **Estimated network cost:** the bootstrap model's optimistic request-wave plus
  transfer estimate. Label it simulated and report profile parameters.
- **Browser benchmark:** a real browser run with raw HTTP request/range evidence,
  cache state, and origin configuration. The deterministic smoke is a
  functional gate; do not call its loopback timing a CDN benchmark.

Report median, p95, maximum, sample count, and failures for a corpus; keep raw
per-query results under `results/` in a documented machine-readable form once a
candidate exists.

## Correctness gates

A performance result is invalid unless both gates pass:

1. the candidate `CanonicalSubgraph` matches the oracle for node IDs/sequences,
   oriented edges, the real reference traversal, and reference coordinates;
2. every selected `CanonicalHaplotypeTile` matches a fresh extraction for its
   exact core/halo, oriented traversals, integer weights, total weight, and
   provenance.

Record domain-separated v1 hashes and a useful semantic diff on mismatch.

## Baselines

At minimum compare source GBZ full load/query, GBZ-base on local SSD, and every
candidate over local positioned reads. When HTTP support exists, serve immutable
objects from a range-capable origin and distinguish cold CDN, warm CDN, browser
cache, and origin responses.

## Experiment modes

For the first run on a multi-gigabyte source, use the encoder-only scale mode:

```bash
cargo run --release -p pangenome-range-cli -- \
  benchmark-encoder-scale <graph.gbz> <run-id> <external-work-root>
```

It builds exactly one archive with the v1 16 KiB/zstd-3 configuration. The
direct-written archive is placed below the supplied work root; there is no
payload spool. Only `config.json`, `summary.json`, and `REPORT.md` are retained
under `results/<run-id>`. This mode deliberately skips GBZ-base and the
20-layout sweep. After encoding, it checks deterministic 1 kb, 10 kb, 100 kb,
and 1 Mb queries against the loaded source graph.
The upstream `gbz` crate still deserializes the source in full; encoder-only
mode bounds archive-construction scratch, not the source graph itself.

The first HPRC whole-genome attempt exposed an unacceptable global occurrence
index. The current archive removed it; encoder-scale metrics must report zero occurrence
bytes/time and a nonzero time to first payload before another whole-genome run.
See the [optimization log](OPTIMIZATION_LOG.md) before repeating that run.

For cheap construction-only pilots, use the public encoder directly:

```bash
cargo run --release -p pangenome-range-cli -- encode INPUT.gbz OUTPUT.pngr \
  --sample GRCh38 --contig chr6 --max-chunks 2 \
  --threads 8 --progress json --report build-report.json
```

The report separates source checksum, source load, reference path index,
manifest discovery, local subgraph selection, record/topology materialization,
binary encode, compression, finalization, validation, and the removed
haplotype-enumeration/copy/occurrence phases.
It also retains input/output SHA-256, time to first payload, archive/index/payload
bytes, queue peaks, scratch/temp bytes, throughput, and process RSS. A filtered
pilot still pays the current full GBZ deserialization and `PathIndex` costs, but
reference-length discovery is restricted before walking reference paths.

For long runs, use `--progress json --progress-interval-seconds 5`. The initial
`encoding_plan` records the exact selected reference count/base total and the
base-window count before adaptive splits. `chunk_progress` events then report
coordinate-based percentage and measured ETA. Do not derive completion from
temporary archive size because regional compression ratios vary.

The accepted whole-source record-preserving run is retained in
[`results/2026-08-25-record-preserving-v4/REPORT.md`](https://github.com/a-r-d/pangenome-range/blob/main/results/2026-08-25-record-preserving-v4/REPORT.md).
It completed the 5.49 GB HPRC v2.1 object in 557.55 seconds end-to-end, including
both SHA-256 passes, versus the stopped materialized-path writer's 84,556-second
ETA (151.65x). Its 475.810-second construction includes a 202.109-second full
payload validation phase; parallel `*_worker_ms` counters are aggregate worker
time, while `payload_pipeline_wall_ms` is elapsed wall time.

Structural validation is necessary but is not a semantic oracle. Verify at
least one retained workload against the exact source after a large build:

```bash
cargo run --release -p pangenome-range-cli -- verify OUTPUT.pngr \
  --against INPUT.gbz --sample CHM13 --contig chr1 \
  --start 1000000 --end 1100000
```

The accepted run checked all seven selected tile-local haplotype results and
matched canonical hash
`cbf983e845fcd6adcb1504089aba3c80fae85cd0c3998bcc90ba02f8fac8c5b4`.

For a full structural reread and a multi-query semantic workload:

```bash
cargo run --release -p pangenome-range-cli -- validate OUTPUT.pngr
cargo run --release -p pangenome-range-cli -- verify OUTPUT.pngr \
  --against INPUT.gbz --workload verification-workload.json \
  --report semantic-validation.json
```

The accepted full archive independently passed all 363,105 physical payloads,
then 9/9 source-oracle queries and 58/58 tile-local haplotype comparisons. The
browser range gate used the same source-verified CHM13 query: Chromium fetched
294,190 bytes in 11 strict `206` responses and decoded seven tiles from the
8.23 GiB object. Chromium, Firefox, and WebKit also pass the deterministic
cross-origin fixture through `pnpm test:browser`. Loopback timing is retained as
functional evidence only. The 11-request large-object observation predates the
current TypeScript coalescing and cache tranche and remains historical evidence,
not a prediction of the current request plan.

## TypeScript reader conformance result

The current Node integration serves a deterministic 164,259-byte v1 archive made
from the pinned 73,920-byte MICB/KIR3DL1 fixture through a real loopback range
origin. It validates one usable `HEAD`, stable `ETag`/`If-Range`, every exact
`206`, the raw origin ranges, the optional query trace, and matching canonical
results from HTTP, Blob, and positioned file sources.

| Query | Selected tiles | Nodes | Weighted traversals | GETs | Fetched bytes | Canonical hash |
|---|---:|---:|---:|---:|---:|---|
| MICB, `GRCh38#chr6:31498145-31511124` | 2 | 659 | 94 | 2 | 37,797 | `8e081a9b...f8adc` |
| KIR3DL1, `GRCh38#chr19:54816468-54830778` | 2 | 2,231 | 67 | 3 | 68,615 | `4cb74bc4...f1cbb` |

The hermetic browser gate opens a separate 70,022-byte synthetic v1 archive in
Chromium, Firefox, and WebKit. Each engine performs one `HEAD`, then exactly
three `206` GETs totaling 20,602 bytes: a 16 KiB bootstrap, one 4 KiB directory
page, and one 122-byte zstd payload. These loopback results establish transport,
CORS, version dispatch, decompression, and decode behavior; the timings are not
a browser performance claim. Exact commands and raw range lists are retained in
`results/2026-08-25-typescript-reader-conformance/`.

The full comparative matrix remains:

```bash
cargo run --release -p pangenome-range-cli -- \
  benchmark-fixed-windows <graph.gbz> <run-id> [random-queries-per-size]
```

Before paying for all 20 window/compression builds on a new input, run the
current small-query candidate alone: 16 KiB base windows, zstd-3, the v1
arithmetic manifest, an 8 MiB raw-payload cap, and no deduplication:

```bash
cargo run --release -p pangenome-range-cli -- \
  benchmark-fixed-window-smoke <graph.gbz> <run-id> [random-queries-per-size]
```

Smoke mode defaults to 10 deterministic random queries for each available size.
It still builds GBZ and GBZ-base baselines, exercises every coalescing threshold,
and requires canonical correctness for every candidate query. Each threshold
uses one reusable archive reader, cold on its first query and retaining its
bootstrap/leaf cache thereafter. OS page-cache state remains uncontrolled.
Because smoke mode runs only one candidate, it cannot establish a new Pareto
winner or measure the incremental benefit of deduplication.
