# Whole-HPRC range-readable GBZ-base and `.pngr` comparison

Status: **all executed correctness and range-origin gates passed; public SQLite
latency is pending explicit publication approval**.

This is the large result that replaces the MHC smoke as the repository headline.
It uses the complete 5.49 GB HPRC v2.1 source, a freshly constructed 10.05 GB
GBZ-base database, a minimally range-tuned SQLite copy, and the exact 10.84 GB
named-membership `.pngr`. The `.pngr` was served by the public immutable range
origin. Both SQLite files were exercised through a strict loopback HTTP Range
VFS; publishing those HPRC-derived database objects for the matching public
latency run remains approval-gated.

## Objects

| Object | Bytes | Relative to source | Role |
| --- | ---: | ---: | --- |
| HPRC source GBZ | 5,492,627,216 | 1.000x | independent source oracle |
| GBZ-base SQLite | 10,050,412,544 | 1.830x | local database baseline |
| range-tuned GBZ-base SQLite | 10,052,300,800 | 1.830x | static-range competitor |
| named-membership `.pngr` | 10,836,425,558 | 1.973x | local file and public static object |

The `.pngr` is **786,013,014 bytes (7.8%) larger than GBZ-base** on this exact
named-path archive. That is a measured loss. The archive SHA-256 is
`82585cb612effbf414b1c8f38b049bc415876866168ccc929f9a885f06d97b0a`; the
source SHA-256 is
`11d6047f79575ffb83757462484bad134ed20928bd2c8171ec52e35a54976e2b`.
The untouched database SHA-256 is
`a5d9a9ec9f4e56f553741de8b5baf9538ef2d7e4a17493f596b145df97c8e8dd`.
The range-tuned copy SHA-256 is
`5f6d14e12ef4e68ba5823a73f2f93a019f3b8c2b78c8479838defb36fe069fbc`.
Its only schema change is:

```sql
CREATE INDEX gbz_base_paths_region_lookup
ON Paths(sample, contig, haplotype, fragment);
```

That index adds 1,888,256 bytes (0.019%). It changes the upstream path lookup
from `SCAN Paths` plus a temporary order B-tree to a direct composite-index
search. The original database remains untouched.

The complete GBZ-base build command finished in 8 minutes 27.40 seconds,
including 9.37 seconds of release recompilation. It peaked at 12,240,204 KiB
RSS and reported zero swaps under a 35 GiB address-space cap. The output and
timing log live outside the repository under
`/media/ard/eba76579-d702-4ff0-b5dd-eb503a726a4d/pangenome-range-data/benchmarks/2026-08-31-hprc-public-network-gbz-base/`.

## Workload and correctness

The checksum-bound workload has 47 queries: MICB and KIR3DL1, five boundary or
distance anchors, and ten deterministic random intervals at each of 1 kb,
10 kb, 100 kb, and 1 Mb. Every query uses 100 bp context. Its SHA-256 is
`c2a88ba48b0221ed69fc8d7c09c221dedacacf4f2d3a40d44e0faf682aacfaba`.

- GBZ-base ran the complete workload twice against one open database. All 94
  serialized subgraphs were byte-for-byte equal to independently queried source
  GBZ JSON. The timed interval is `Subgraph::from_db`; database open,
  serialization, hashing, and equality checks are excluded.
- Rust verified 47/47 `.pngr` canonical graph hashes and all 730 selected
  tile-local haplotype payloads against the source GBZ. The strict verification
  took 6 minutes 51.47 seconds, peaked at 26,874,868 KiB RSS, and reported zero
  swaps; it is validation cost, not query latency.
- The Node reader passed all 188 public-network scenarios and all 94 local-file
  scenarios. The public origin separately passed HEAD, CORS, stable ETag,
  immutable/no-transform cache policy, and four exact byte-for-byte `206` probes.

GBZ-base uses its all-haplotypes, no-snarls JSON semantics. `.pngr` uses the
documented exact graph plus weighted tile-local haplotypes and named-membership
extension. Those outputs are related but not identical models, so each is checked
against its declared source semantics rather than falsely presented as identical
serialization.

## Static HTTP Range result

The HTTP SQLite harness registers a read-only VFS, then runs the unchanged
upstream `GBZBase::open`, `GraphInterface`, and `Subgraph::from_db` code. It
strictly validates `206`, `Content-Range`, response length, and stable `ETag`;
requests use `Cache-Control: no-store`. Every workload query uses a fresh
SQLite connection, HTTP client, and 32 MiB byte-bounded VFS cache. Thus the
request and byte counts are actual HTTP transport work, not a projection from
SQL query plans or `pread` traces.

A 4 KiB policy matches the database page size and minimizes overfetch. A sweep
on MICB showed why it is not automatically the best network policy:

| Fetch chunk | Requests including HEAD | Response bytes |
| --- | ---: | ---: |
| 4 KiB | 36 | 143,360 |
| 16 KiB | 19 | 294,912 |
| 64 KiB | 15 | 917,504 |
| 256 KiB | 14 | 3,407,872 |
| 1 MiB | 12 | 11,534,336 |

The retained full-workload competitor therefore includes 64 KiB as a practical
request/overfetch point, while retaining 4 KiB results and the untouched schema
so the choice remains auditable.

| Random interval | upstream SQLite 4 KiB requests / bytes p50 | tuned SQLite 4 KiB requests / bytes p50 | tuned SQLite 64 KiB requests / bytes p50 | `.pngr` public requests / bytes p50 |
| --- | ---: | ---: | ---: | ---: |
| 1 kb | 679 / 2.78 MB | 21 / 81.9 KB | 14 / 852.0 KB | 2 / 25.8 KB |
| 10 kb | 687 / 2.81 MB | 29 / 114.7 KB | 15 / 917.5 KB | 2 / 46.1 KB |
| 100 kb | 753 / 3.08 MB | 95 / 385.0 KB | 20 / 1.25 MB | 2 / 196.4 KB |
| 1 Mb | 1,444 / 5.91 MB | 786 / 3.22 MB | 64 / 4.13 MB | 2 / 1.57 MB |

Across all 47 cold queries:

| Layout / policy | HTTP requests | Response bytes | Source-oracle result |
| --- | ---: | ---: | ---: |
| upstream SQLite, 4 KiB | 40,801 | 166,928,384 | 47/47 exact JSON hashes |
| tuned SQLite, 4 KiB | 9,875 | 40,255,488 | 47/47 exact JSON hashes |
| tuned SQLite, 64 KiB | 1,249 | 78,774,272 | 47/47 exact JSON hashes |
| `.pngr`, public cold WASM | 94 | 18,626,490 | 47/47 canonical graphs |

Against the range-tuned 64 KiB competitor, `.pngr` uses 13.3x fewer requests
and 4.23x fewer bytes. Against tuned 4 KiB SQLite, it uses 105x fewer requests
and 2.16x fewer bytes. These are the format-layout result. The SQLite origin was
loopback, so this section intentionally does not compare its wall time to the
public `.pngr` wall time.

## Random-query result

Each row below contains ten fixed-seed random intervals. `p95` uses the retained
nearest-rank convention and therefore equals the maximum for a ten-query class;
the median is the more stable headline. GBZ-base is its first local pass with
uncontrolled/mixed OS cache. Local `.pngr` is a new reader per query. Public
`.pngr` is real HTTP with `no-store`, a new reader per query, the WASM zstd
decoder, a 1 MiB directory cache, and a 32 MiB compressed-payload cache.

| Interval | GBZ-base local p50 / p95 | `.pngr` local p50 / p95 | `.pngr` public p50 / p95 | Public bytes p50 / p95 |
| --- | ---: | ---: | ---: | ---: |
| 1 kb | 6.33 / 998.06 ms | 84.74 / 6,433.98 ms | 384.26 / 6,063.00 ms | 25,847 / 402,128 |
| 10 kb | 33.54 / 74.94 ms | 140.89 / 314.35 ms | 428.22 / 610.35 ms | 46,117 / 66,191 |
| 100 kb | 517.71 / 1,992.22 ms | 719.85 / 2,861.75 ms | 1,024.99 / 3,319.19 ms | 196,383 / 481,072 |
| 1 Mb | 8,617.59 / 15,117.73 ms | 6,652.09 / 13,187.15 ms | 7,397.72 / 14,269.19 ms | 1,570,680 / 2,321,292 |

GBZ-base wins the 1 kb, 10 kb, and 100 kb local medians. `.pngr` wins at 1 Mb:
its local p50 is 1.30x faster and local p95 is 1.15x faster. Even after the real
public route, its 1 Mb p50 is 1.16x faster and p95 is 1.06x faster than the
local GBZ-base baseline while fetching 1.57/2.32 MB p50/p95 from a 10.84 GB
object. That cross-environment comparison is useful deployment evidence, not a
claim that WAN and local SQLite are interchangeable microbenchmarks.

Every cold public query issued exactly two actual HTTP requests. Across all 47
cold WASM queries the reader transferred 18,626,490 bytes. The public matrix also
ran pure JavaScript and warm repeated-query cases: WASM reduced per-chunk
decompression p95 from 214.53 ms to 11.24 ms, but decoding and graph
reconstruction dominate dense regions.

## Commands

The internal GBZ-base helpers exist only to make this retained comparison
reproducible; they are not part of the public `.pngr` encoder path.

```bash
prlimit --as=37580963840 -- cargo run --release \
  -p pangenome-range-cli -- internal-gbz-base-build \
  SOURCE.gbz EXTERNAL_WORK_ROOT/hprc-v2.1-mc-grch38.gbz.db

prlimit --as=37580963840 -- cargo run --release \
  -p pangenome-range-cli -- internal-gbz-base-workload \
  SOURCE.gbz EXTERNAL_WORK_ROOT/hprc-v2.1-mc-grch38.gbz.db \
  results/2026-08-31-hprc-public-network-gbz-base/workload.json \
  results/2026-08-31-hprc-public-network-gbz-base/gbz-base-summary.json

cargo run --release -p pangenome-range-cli \
  --features remote-sqlite-benchmark -- \
  internal-gbz-base-http-workload SQLITE_URL \
  results/2026-08-31-hprc-public-network-gbz-base/workload.json \
  results/2026-08-31-hprc-public-network-gbz-base/gbz-base-summary.json \
  results/2026-08-31-hprc-public-network-gbz-base/sqlite-http.json \
  65536 33554432

node packages/benchmark/dist/cli.js archive \
  --url PUBLIC_ARCHIVE_URL \
  --workload results/2026-08-31-hprc-public-network-gbz-base/workload.json \
  --run-id hprc-public-network --mode both --decoder both \
  --http-cache no-store
```

The retained directory contains the workload, origin validation, local and
network raw request/query tables, GBZ-base per-query hashes and timings, strict
Rust verification, process resource reports, and
`comparison-summary.json`.

## Limitations

- Ten random queries per size are enough to expose the scale crossover but not
  to estimate tail percentiles precisely; the reported class p95 is the maximum.
- The public run is Node 24 from one host and route, not a controlled
  multi-location browser benchmark. CDN and kernel cache state are uncontrolled.
- GBZ-base does not ship a static HTTP VFS. This experiment supplies an
  explicit read-only VFS and cache policy without changing its graph query.
  The SQLite request/byte evidence is real loopback HTTP; the matching public
  route remains unmeasured until publishing the two HPRC-derived database
  objects is explicitly approved.
- Construction wall times are not compared. The `.pngr` object was built and
  extended in retained stages, while GBZ-base was constructed in one fresh run.
- The format still lacks first-class snarl semantics and exact cross-tool
  coordinate interoperability; this benchmark does not erase those product gaps.
