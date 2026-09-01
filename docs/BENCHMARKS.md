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

## Retained GBZ-base comparison (2026-08-31)

The headline comparison uses the complete 5,492,627,216-byte HPRC v2.1 source
GBZ, SHA-256
`11d6047f79575ffb83757462484bad134ed20928bd2c8171ec52e35a54976e2b`.
A fresh upstream GBZ-base build produced a 10,050,412,544-byte SQLite database.
The complete command took 8 minutes 27.40 seconds, including 9.37 seconds of
release recompilation, at 12,240,204 KiB peak RSS and zero swaps. The exact
published named-membership `.pngr` is 10,836,425,558 bytes with SHA-256
`82585cb612effbf414b1c8f38b049bc415876866168ccc929f9a885f06d97b0a`.
It is 7.8% larger than GBZ-base on this corpus.

The checksum-bound workload contains 47 queries: two fixed biological loci,
five boundary/distance anchors, and ten fixed-seed random intervals at each of
1 kb, 10 kb, 100 kb, and 1 Mb. GBZ-base ran the workload twice; all 94 outputs
were exactly equal to independently generated source-GBZ JSON. Rust separately
verified all 47 `.pngr` canonical graphs and all 730 selected tile-local
haplotype payloads against the source. Node passed all 188 real public-network
and 94 local-file scenarios.

| Random interval | GBZ-base local p50 / p95 | `.pngr` local p50 / p95 | `.pngr` public p50 / p95 | Public bytes p50 / p95 |
| --- | ---: | ---: | ---: | ---: |
| 1 kb | 6.33 / 998.06 ms | 84.74 / 6,433.98 ms | 384.26 / 6,063.00 ms | 25,847 / 402,128 |
| 10 kb | 33.54 / 74.94 ms | 140.89 / 314.35 ms | 428.22 / 610.35 ms | 46,117 / 66,191 |
| 100 kb | 517.71 / 1,992.22 ms | 719.85 / 2,861.75 ms | 1,024.99 / 3,319.19 ms | 196,383 / 481,072 |
| 1 Mb | 8,617.59 / 15,117.73 ms | 6,652.09 / 13,187.15 ms | 7,397.72 / 14,269.19 ms | 1,570,680 / 2,321,292 |

GBZ-base wins the smaller local classes. At 1 Mb, `.pngr` is 1.30x faster at
the local median and 1.15x faster at local p95. The real public-range median is
still 1.16x faster than local GBZ-base while transferring 1.57 MB from the
10.84 GB object in exactly two actual requests. The public run used Node 24,
`no-store`, the WASM decoder, a 1 MiB directory cache, and a 32 MiB compressed
payload cache. It is real HTTP evidence from one host and route, not a browser
or multi-location CDN benchmark. Each random class has ten queries, so the
nearest-rank p95 equals the class maximum.

GBZ-base does not ship a static HTTP range VFS, but the retained comparison now
supplies an opt-in read-only VFS around the unchanged upstream query. The
range-tuned 10,052,300,800-byte copy adds only
`Paths(sample, contig, haplotype, fragment)`, increasing size by 1,888,256 bytes
(0.019%) and removing an otherwise avoidable full `Paths` scan.

Across the 47 cold queries, the tuned SQLite file at the retained 64 KiB fetch
policy used 1,249 actual HTTP requests and 78,774,272 response bytes. The public
`.pngr` cold-WASM run used 94 requests and 18,626,490 bytes: 13.3x fewer
requests and 4.23x fewer bytes. A 4 KiB SQLite policy reduced bytes to
40,255,488 but required 9,875 requests. All three SQLite runs matched 47/47
retained source-GBZ JSON hashes. SQLite timings are loopback and are not called
public-network results; publishing the checksum-bound HPRC-derived databases is
approval-gated.

The two formats also expose different declared haplotype models, so both are
checked independently against source semantics rather than falsely described
as byte-identical outputs. Commands, raw requests, process reports, workload,
machine-readable comparison, and the full limitations are retained under
`results/2026-08-31-hprc-public-network-gbz-base/`.

### Medium local control

The closing comparison uses the checked-in 4,511,832-byte `mhc-10.gbz` corpus,
SHA-256
`a0b44236852d5659202a6855308020df05efd7c2be90645d341d94fb775df685`.
The smoke command built a 10,301,440-byte GBZ-base SQLite database and a
4,807,424-byte current-v1 16 KiB/zstd-3 `.pngr`, then ran 200 deterministic
queries with 100 bp context: 50 each at 1 kb, 10 kb, 100 kb, and 1 Mb.
GBZ-base output matched the loaded GBZ. All 1,200 `.pngr` measurements across
six coalescing settings passed both canonical graph and exact weighted
tile-local haplotype gates, including 21,546 freshly checked tile payloads.

| Query class | GBZ-base p95 | `.pngr` p95 | `.pngr` relative to GBZ-base |
| --- | ---: | ---: | ---: |
| 1 kb | 2.94 ms | 16.05 ms | 5.47x slower |
| 10 kb | 14.49 ms | 49.02 ms | 3.38x slower |
| 100 kb | 287.23 ms | 295.08 ms | 1.03x slower |
| 1 Mb | 1,336.85 ms | 1,171.80 ms | 1.14x faster |
| Balanced 200-query workload | 1,249.00 ms | 1,069.58 ms | 1.17x faster |

The `.pngr` archive was 53.3% smaller than the GBZ-base database and 6.6%
larger than the source GBZ. Its p95 fetched bytes were 33,881, 89,591, 413,945,
and 1,762,610 across the four query-size classes. GBZ-base won small-query local
latency, the 100 kb class was close, and `.pngr` crossed ahead at 1 Mb. This is
a measured crossover on a medium fixture, not a projection of chromosome-scale
performance.

```bash
cargo run --release -p pangenome-range-cli -- \
  benchmark-fixed-window-smoke test-data/mhc-10.gbz \
  2026-08-31-gbz-base-mhc-comparison 50
```

The complete config, environment, machine-readable summary, raw query table,
and report are retained under
`results/2026-08-31-gbz-base-mhc-comparison/`.

### Tiny upstream-fixture control

The tiny control uses the exact 73,920-byte `micb-kir3dl1.gbz` fixture
published with GBZ-base, SHA-256
`1d574ede7533150eb87f6837a7763d4eac120aa03f34877392ecdd53b0410788`.
The smoke command built a 172,032-byte GBZ-base SQLite database and a
165,052-byte current-v1 16 KiB/zstd-3 `.pngr`, then ran 102 deterministic
queries with 100 bp context. GBZ-base output matched the loaded GBZ, and all 612
`.pngr` measurements across six coalescing settings passed both canonical graph
and exact weighted tile-local haplotype gates.

| Local measurement | GBZ-base | `.pngr` | `.pngr` relative to GBZ-base |
| --- | ---: | ---: | ---: |
| Stored bytes | 172,032 | 165,052 | 4.1% smaller |
| MICB, `GRCh38#chr6:31498145-31511124` | 19.61 ms | 35.19 ms | 1.79x slower |
| KIR3DL1, `GRCh38#chr19:54816436-54830779` | 67.93 ms | 105.73 ms | 1.56x slower |
| 102-query p95 | 50.51 ms | 104.62 ms | 2.07x slower |

The `.pngr` MICB and KIR3DL1 queries fetched 37,797 and 52,327 bytes in two
positioned reads. GBZ-base won local latency; `.pngr` was slightly smaller and
retained its static-object range contract. Timings used release Rust code with
uncontrolled OS page-cache state, exclude correctness serialization and
comparison work, and are not cold-storage, browser, or public-network results.
The fixture cannot exercise the skipped 100 kb and 1 Mb classes or establish a
chromosome-scale projection.

```bash
cargo run --release -p pangenome-range-cli -- \
  benchmark-fixed-window-smoke test-data/micb-kir3dl1.gbz \
  2026-08-31-gbz-base-fixture-comparison 50
```

This tiny run is retained because it uses GBZ-base's own fixture, but fixed
archive metadata and small-query decoding dominate it. It must not be used as
the repository's headline performance result. The complete config,
environment, machine-readable summary, raw query table, and report are under
`results/2026-08-31-gbz-base-fixture-comparison/`.

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
The public encoder now defaults to project-owned disk-backed GBZ access. The
older encoder-scale research command still uses its loaded graph because it
also runs loaded source-oracle queries; do not use that command to measure the
new source adapter.

The first HPRC whole-genome attempt exposed an unacceptable global occurrence
index. The current archive removed it; encoder-scale metrics must report zero occurrence
bytes/time and a nonzero time to first payload before another whole-genome run.
See the [optimization log](OPTIMIZATION_LOG.md) before repeating that run.

For cheap construction-only pilots, use the public encoder directly:

```bash
cargo run --release -p pangenome-range-cli -- encode INPUT.gbz OUTPUT.pngr \
  --sample GRCh38 --contig chr6 --max-chunks 2 \
  --source-access disk --scratch-dir EXTERNAL_SCRATCH \
  --threads 8 --progress json --report build-report.json
```

The schema-6 report separates source checksum, source load or disk-cache build,
their combined critical-path wall, reference path index,
manifest discovery, local subgraph selection, record/topology materialization,
binary encode, compression, finalization, validation, and the removed
haplotype-enumeration/copy/occurrence phases.
It also retains input/output SHA-256, time to first payload, archive/index/payload
bytes, source-cache files and byte limits, queue peaks, scratch/temp bytes,
throughput, and process RSS. A filtered disk-backed pilot still builds the
whole source cache and compact real-reference index before the first payload;
filters do not make that preprocessing partial.
Source SHA-256 overlaps source preparation and is labeled as worker wall rather
than an additive phase. The report times the final output SHA-256 and records
the complete pre-report wall critical path; worker CPU milliseconds remain
separate.

For long runs, use `--progress json --progress-interval-seconds 5`. The initial
`encoding_plan` records the exact selected reference count/base total and the
base-window count before adaptive splits. `chunk_progress` events then report
coordinate-based percentage and measured ETA. Do not derive completion from
temporary archive size because regional compression ratios vary.
`source_checksum_progress` and `output_checksum_progress` report byte-based
completion, opaque source-load/index phases emit heartbeats, and
`path_membership_validation_progress` reports validated membership tiles,
groups, bytes, rate, elapsed time, and ETA while named-membership pages are
reconciled with their regional payloads. `archive_validation_progress` then
reports validated directory entries/pages, physical payloads, bytes, rate,
elapsed time, and ETA for the regional-payload stage alone. A long run should
never have a silent phase merely because payload encoding has finished.

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
cargo run --release -p pangenome-range-cli -- validate OUTPUT.pngr \
  --mode standard --workers 8 --max-queued-bytes 536870912
cargo run --release -p pangenome-range-cli -- validate OUTPUT.pngr \
  --mode full --workers 8 --max-queued-bytes 536870912
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

The release-candidate whole-HPRC rerun is retained in
[`results/2026-08-26-format-v1-release-candidate/REPORT.md`](https://github.com/a-r-d/pangenome-range/blob/main/results/2026-08-26-format-v1-release-candidate/REPORT.md).
On the same source/options/host, the standard pre-rename gate fell from
240.736 s to 28.123 s with eight bounded workers. The archive and index sizes
remained exactly 8,828,788,418 and 47,376,617 bytes. This is structural and
integrity evidence; the separate nine-query GBZ oracle supplied semantic
evidence.

The project-owned disk-source whole-HPRC run is retained in
[`results/2026-08-26-bounded-disk-source-v1/REPORT.md`](https://github.com/a-r-d/pangenome-range/blob/main/results/2026-08-26-bounded-disk-source-v1/REPORT.md).
It produced the identical 8,828,788,418-byte archive and SHA-256 while reducing
peak RSS from 8,775,928 to 608,060 KiB. Whole wall increased from 438.72 to
499.34 seconds on the exact same source/options/host; the 11,921,858,427-byte
ephemeral source cache is reported separately from zero encoder scratch.

The canonical GENCODE v50 named-locus run is retained in
[`results/2026-08-26-gencode-v50-whole-hprc/REPORT.md`](https://github.com/a-r-d/pangenome-range/blob/main/results/2026-08-26-gencode-v50-whole-hprc/REPORT.md).
On the same whole-HPRC source/options/host as the unified-worker baseline, all
78,733 CHR genes became 157,466 symbol/stable-ID records in 612 leaves. The
archive grew by 3,719,573 bytes (+0.0421%); cold exact search requires the
61,145-byte descriptor and one 6.9-8.0 KiB leaf in two dependency rounds.

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

## TypeScript/Node and real-browser harness

`packages/benchmark` is a private Node 24 package. Its workload is a JSON object
with `schemaVersion: 1`, the exact archive SHA-256, a fixed seed, and query
records containing class, reference coordinates, context, and exactly one
expected canonical hash or expected error category. The generator includes
covered MICB/KIR3DL1 loci, start/end boundaries, deterministic 1 kb/10 kb/100
kb/1 Mb intervals when a reference is long enough, nearby/distant queries, and
an absent-reference case. Rust `verify --workload` accepts the same wrapper,
checks the archive checksum and all positive expected hashes, and explicitly
reports reader-specific negative cases it skips. Legacy Rust query arrays remain
accepted for retained historical workloads.

The CLI is:

```bash
pnpm bench -- workload --file ARCHIVE.pngr --output workload.json
pnpm bench -- archive --file ARCHIVE.pngr --workload workload.json --run-id RUN
pnpm bench -- archive --url URL --workload workload.json --run-id RUN
pnpm bench -- browser --file ARCHIVE.pngr --workload workload.json --run-id RUN
pnpm bench -- origin-check --url URL [--origin PAGES_ORIGIN] [--file ARCHIVE.pngr] [--sha256 HEX]
pnpm bench -- compare --runs RUN_A RUN_B [--rust-summary verification.json]
```

Every archive/browser run exclusively creates `results/<run-id>/`; it refuses
to overwrite an existing directory. It writes `config.json`,
`environment.json`, `requests.ndjson`, `queries.csv`, `summary.json`, and
`REPORT.md`. Environment evidence includes git status/SHA, Node/pnpm/package and
browser versions, host/CPU/memory, archive identity, cache budgets, HTTP cache
policy, and limitations. Per-query evidence keeps actual range-origin/source
requests and bytes separate from planned reader ranges/bytes and dependency
rounds. Browser runs record reader-observed fetches, origin-observed request
rounds, and an explicit reconciliation result; a mismatch fails correctness.
They also record cache hits, decompression/decode/merge/total time, canonical
hash/correctness, and available heap observations.

The local archive origin supports multiple immutable routes, `HEAD`, exact
single byte ranges, CORS and exposed range headers, identity encoding,
`no-transform`, stable ETags, and request logs containing method, range, status,
bytes, elapsed time, and connection ID. Controlled modes cover ignored ranges,
malformed `Content-Range`, truncated bodies, changed ETags, latency, bandwidth,
missing CORS, and missing exposed headers. Multipart ranges are deliberately
absent because the reader does not issue them.

The real-browser matrix runs the built public `pangenome-range/reader` entry in
Chromium, Firefox, and WebKit. It measures:

1. cold library cache with transport `no-store`;
2. cold library cache with normal browser HTTP caching;
3. warm directory cache with the compressed-payload cache disabled;
4. a repeated exact query with both library caches retained;
5. a nearby pan query;
6. a distant random query.

Fresh ephemeral Playwright contexts define the two cold cases. This controls
context cache state but not operating-system caches. Warm library-cache cases
force transport `no-store`, and actual origin logs are reconciled with reader
traces and Performance API measurements. Loopback latency remains qualified as
local functional evidence.

The default decoder remains bundled pure-JavaScript `fzstd`. The optional
`@bokuweb/zstd-wasm` adapter implements the same `ChunkDecompressor` interface
and is benchmark-only. Reports include initialization, unique JavaScript/WASM
asset bytes, each decompression call, p50/p95, total query latency, correctness,
and memory when the runtime exposes it. The WASM module is initialized once per
page; reinitializing its singleton between archive instances corrupts later
calls and is a rejected lifecycle.

`origin-check` probes metadata, the `GET`/`Range`/`If-Range` CORS preflight, and
small, overlapping, adjacent, and tail ranges. It fails on `200` full-object
fallbacks, malformed metadata, unstable ETags, transformed bodies, missing
CORS/exposed headers, and local byte mismatches. A checksum without a local
file requires a content-addressed ETag;
provide `--file` for exact sampled-byte comparison. `--origin` (or
`PANGENOME_RANGE_CORS_ORIGIN`) supplies the actual GitHub Pages origin whose
CORS grant is being tested; the default is an intentionally unrelated test
origin. No deployment URL is hardcoded.

The comparison command presents Rust planned rounds/ranges/bytes and simulated
20 ms network cost in a separate section from real Node/browser source/origin
requests, local runtime decode, and cold/warm end-to-end latency. Default CI
installs Chromium and runs the pure-JS smoke subset. The three-engine,
two-decoder matrix and large archives remain manual/scheduled work.

## Retained benchmark-harness evidence (2026-08-25)

The current-format conformance archive and the locally encoded MHC archive were
run through both decoders and all three installed Playwright engines. The
retained evidence is:

- `results/2026-08-25-node-benchmark-harness-smoke/REPORT.md`: 40/40 Node
  cold/warm conformance queries passed;
- `results/2026-08-25-browser-benchmark-harness-smoke/REPORT.md`: 36/36
  conformance browser scenarios passed;
- `results/2026-08-25-node-mhc-benchmark-smoke/REPORT.md`: 56/56 Node MHC
  queries passed, including 1 kb, 10 kb, 100 kb, and 1 Mb intervals;
- `results/2026-08-25-browser-mhc-benchmark-smoke/REPORT.md`: 36/36 MHC browser
  scenarios passed, with the matching Rust verification and comparison in the
  same directory. `archive-build.json` retains the source checksum and encoder
  construction metrics.

The retained build configuration can be reproduced with:

```bash
cargo run --release -p pangenome-range-cli -- encode \
  test-data/mhc-10.gbz /tmp/pangenome-range-mhc-benchmark-v1.pngr \
  --sample MHC-GRCh38 --contig MHC --threads 8 --progress off \
  --report /tmp/pangenome-range-mhc-benchmark-build.json
```

The browser matrix used Chromium 151.0.7922.34, Firefox 153.0, and WebKit 26.5.
The MHC archive was 4,806,677 bytes with SHA-256
`ec71bdfff9e0ebdf5bbbac9dcb77b547ba334054b5980d17cceba1c29509e1c5`.
Its 13 positive workload queries matched Rust and the one reader-specific
absent-reference case was explicitly skipped by Rust. Browser pure-JS chunk
decompression was 2.9/5.0 ms p50/p95; WASM was 1.0/2.0 ms after 6 ms
initialization and added 251,806 WASM bytes. These are loopback functional and
local-performance results, not CDN or public-network claims.

No current-v1 chromosome-scale or multi-GB archive was available. Historical
large archives use incompatible pre-reset bytes and cannot be relabeled as v1.
The next evidence tranche is a current-v1 chromosome/GB-scale archive served by
the configured public range origin, using this exact checksum-bound workload
and origin validator.

## Retained explorer evidence (2026-08-27)

`results/2026-08-27-viewer-explorer-v1/` closes that evidence gap with the
8,832,749,949-byte current-v1 GENCODE/HPRC object. Chromium ran the retained
100 kb CHM13 query through both pure-JavaScript and optional WASM zstd across
the six cache scenarios. All 12 canonical hashes passed and every reader trace
reconciled with the strict loopback origin. Cold no-store pure-JS query wall was
1,565.3 ms versus the prior 1,678.3 ms; WASM measured 1,458.1 ms. The 2x goal
was not met because regional decode/reconstruction remains the critical phase.

The same tranche retains public-origin exact-range/byte validation, one cold
and warm native `HLA-B` search, a bounded/truncated prefix search, an absent
search, cold and warm summary queries, Chromium/Firefox/WebKit application
smoke, and desktop light/dark/tablet screenshots. See
[Viewer performance](VIEWER_PERFORMANCE.md) for the phase definitions and
limitations.

## Explorer redesign validation (2026-08-27)

`results/2026-08-27-explorer-redesign-v1/` retains the canvas-first explorer
screenshots and the hermetic Pages result after the orientation-first redesign.
The run exercised 12 real `206` responses, custom/local source switching,
cancel/recovery, actionable malformed-range handling, browser history, and
Chromium/Firefox/WebKit. It also asserts that the document itself does not
overflow at 1600x1000, 1366x768, 1024x768, or 820x1180; scrolling remains
inside explicit popovers, inspectors, and the evidence drawer.

The deterministic 4.3 KiB fixture measured 5.8 ms object open, 0.6 ms first
summary paint, 1.2 ms first graph-tile paint, 1.9 ms query completion, and 0.3
ms canvas paint in one Chromium loopback run. These values validate UI phase
instrumentation only; they do not replace the retained multi-gigabyte public
archive measurements above.

## Single-screen tube-map browser (2026-08-27)

`results/2026-08-27-simple-browser-v1/` supersedes the production demo UI with
a single-screen reference-anchored SVG tube map. It retains the before/after
component and bundle inventory, deterministic golden screenshot, public HLA-B
and lower-complexity screenshots, exact byte-range/query status, and the
Chromium/Firefox/WebKit result. Older explorer reports remain historical
evidence and are not the current product contract.

## Bounded integrated path-membership proof (2026-08-28)

`results/path-membership/integrated-encoder/` retains the first same-object encoder
proof. One- and four-worker tiny archives are byte-identical. The real HPRC workload
is four aligned TERT tiles, not a chromosome or whole-genome run: the direct encoder
peaked at 263,828 KiB RSS with zero swaps under a 4 GiB address-space cap, and full
validation reconciled all 1,686 named groups with their unchanged anonymous payloads.
Chromium, Firefox, and WebKit each recovered 464 exact catalog records through four
strict `206` requests totaling 76,869 bytes. Loopback timing is functional evidence,
not public-network performance.

## Production named-membership conformance (2026-08-29)

The production tranche changes placement, so earlier archive SHA values remain
historical. A checked-in MICB/KIR3DL1 fixture now covers two CHM13 chr6 tiles. Its
32,013-byte archive has SHA-256
`5900f5c77ef4ee46a1e22e51f30634f95a0bf5c50b00e40ef75b80026cb00955`.
Rust and TypeScript agree on 169 catalog paths, 79 groups, 180 memberships, and total
occurrence/multiplicity weight 180. Browser tests also corrupt the fixed membership
directory body and require digest rejection.

The synthetic direct encode through ephemeral and persistent source-cache v2 remains
the cache-path determinism gate. Final-layout anonymous/named controls, validation,
query bytes, and memory evidence are retained under `results/named-membership/`.
Rice and four-tile HPRC retain anonymous controls. A later local Populus trichocarpa
Chr19 run expands the named-only construction proof to 1,015 tiles and full
reconstruction under a 4 GiB, zero-swap cap; it has no anonymous size-control run and
is not published pending data-use review. The permanent 1000G operational exclusion
is explicit, the current direct encoder was not attempted there, and no result is
inferred.
