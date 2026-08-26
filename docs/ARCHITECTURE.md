# Architecture

## Dependency direction

```text
pangenome-range-cli ──> pangenome-range-build ──> pangenome-range-query
     │                         │                         │
     └────> upstream gbz       └────────> pangenome-range-format
                               └ - - -> gbz-base (research/oracle only)
```

`pangenome-range-format` owns the normative v1 header/root/directory and
regional codecs, corruption checks, reader primitives, structural validation,
storage seam, and network cost model. It has no GBZ dependency.
`pangenome-range-query` owns canonical graph/tile semantics, comparison, and
hashing. `pangenome-range-build` owns GBZ source access, reference anchoring,
tile selection, the bounded encoder pipeline, build metrics, and candidate-layout
experiments. `gbz-base` remains only in research baselines and independent
source-oracle checks; it no longer defines or constructs production payloads.
The current object is normatively specified
in [File Format v1](FILE_FORMAT_V1.md) and summarized in
[Fixed-window archive v1](FIXED_WINDOW_ARCHIVE.md).

## Direct archive construction

The normal v1 encoder writes a temporary sibling of the requested `.pngr`
object. It emits the provisional 64-byte header and root, reserves the exact
fixed directory-page span, and then appends accepted payloads in deterministic
reference/coordinate order. Compact per-reference/per-bucket descriptors are
retained for directory backfill; there is no payload spool, second full-file
copy, occurrence table, or global pending-entry sort.

The normal regional path uses the project-owned `LocalSubgraph` selector. It
copies exact packed GBWT records, forward node sequences, and canonical topology
edges directly from `PangenomeSource`; it does not call `gbz-base`, enumerate,
or sort anonymous paths while encoding. Exact record lengths provide the
adaptive-split preflight,
so an oversized parent is rejected before a second payload corpus is copied.
The real reference is retained as a GBWT occurrence anchor with fragment and
node offsets. Weighted anonymous paths are reconstructed only by a reader for
the tiles selected by a query.

Tile selection/materialization and compression use bounded batches controlled
by `--threads` and `--max-queued-bytes`; the CLI defaults to available
parallelism capped at eight and a 256 MiB queue cap. Results are consumed in
input order, and adaptive children precede later completed work, so worker
completion order cannot change archive offsets or bytes. The completed
temporary object is flushed, then the standard gate validates directory/range
structure, BLAKE3-128 over every unique encoded payload, exact decompression,
and regional structural decoding with bounded workers. Only then is it
atomically renamed. `validate --mode full` additionally reconstructs every
physical tile traversal; `verify` remains the independent selected-query source
oracle. Failed temporary objects are removed unless `--keep-partial` is set.

The encoder computes exact total selected reference bases before payload work.
Progress snapshots report current reference/coordinate, accepted and physical
chunks, processed/total bases, global and per-reference percentage, observed
bp/s and chunks/s, a rate-derived ETA, build/processing elapsed time, and the
current temp-final byte length. JSON and plain progress contain the same
measurements; `--progress-interval-seconds` controls their cadence. Percent is
coordinate-based rather than inferred from compressed bytes.

Progress covers every potentially long CLI phase. Input and output checksum
passes report bytes, percentage, transfer rate, elapsed time, and ETA. GBZ load
and compact path-index construction emit elapsed-time heartbeats because the
upstream APIs do not expose reliable partial-work counts. The final structural
validation reports directory entries/pages and unique physical payloads,
compressed bytes reread, percentage, rate, elapsed time, and ETA. Interactive
terminals select readable plain progress by default; JSON mode emits stable
newline-delimited events, while redirected commands remain quiet unless a mode
is selected explicitly.

Wall phases in encoder reports are non-overlapping through the output SHA-256
pass. Aggregate selection, encoding, compression, decode, and reconstruction
worker milliseconds are labeled separately and must not be added to the wall
critical path.

## Source access seam

`PangenomeSource` isolates reference discovery, owned active node/record access,
and reference-position lookup from the encoder pipeline. `DiskGbzSource` is the
default encoder adapter. It parses GBZ/simple-sds sections directly, streams the
packed-record body and decoded concatenated node sequences into four ephemeral
files, writes arithmetic offset tables, and reads them through four 16 MiB
block caches. Each cache has 16 fixed shards so bounded workers do not serialize
on one cache lock; the total explicit read-cache limit remains 64 MiB. The
source cache is removed on exit and is independent of the atomic `.pngr`
temporary sibling.

GBZ v1 stores 425,853,421 record offsets and 212,926,710 sequence offsets for
the retained HPRC source. The cache therefore intentionally trades 11.92 GB of
temporary disk for a measured 608,060 KiB whole-encode process peak instead of
fully loading the 5.49 GB GBZ at an 8,775,928 KiB peak. The compact simple-sds indices used
while constructing the cache still scale with record/sequence count; “bounded”
means the source body and active reads are not retained in RAM, not constant
memory independent of graph metadata.

`SourcePathIndex` is project-owned and samples only real reference paths roughly
every 1,000 bp using length-only sequence lookup. `LocalSubgraph` performs interval walking and bidirectional
context expansion from packed records. Neither is a global haplotype occurrence
index. `LoadedGbzSource` remains available via `--source-access loaded` as the
byte-correctness baseline.

## Primary npm product boundary

`packages/browser` is the primary npm package. It publishes isolated reader,
viewer, and Node library entries plus a separate Node executable shim. The root
and `/reader` entries contain no DOM, Node built-ins, launcher logic, or native
code; `/viewer` owns the framework-neutral rendering contract, `/node` owns
Node-only range sources, and `bin/` owns native process selection and launch.
Adding the executable therefore does not change the reader/viewer dependency
graph or bundle-size budgets. `packages/benchmark` remains private and owns Node
and real-browser measurements.

The shim maps the runtime operating system, architecture, and Linux libc to one
exact-version optional `@pangenome-range/cli-*` package. Those platform packages
are generated only during release staging and contain no JavaScript runtime,
installer, downloader, or compiler. The shim validates package/binary metadata,
then spawns the Rust CLI with inherited standard streams and forwarded arguments,
signals, and exit status. Missing or unsupported native packages do not prevent
ordinary library imports.

The public contracts, strict HTTP/Blob/memory/file range sources, v1
bootstrap/root and arithmetic directory reader, byte-bounded caches,
pure-JavaScript zstd decompressor, record-preserving regional decoder, and
canonical graph assembly exist. Rust and TypeScript decode the same current
fixture into typed-array-oriented nodes, sequences, topology, real reference
traversal, and weighted tile-local paths, then produce identical canonical
hashes. A bounded progressive Canvas 2D viewer consumes only `queryTiles()` and
the query trace contract. A public-network browser benchmark corpus remains
explicitly unimplemented.

The benchmark package wraps the public reader with a versioned workload, Node
and Playwright runners, a strict/fault-injectable local range origin, immutable
raw result retention, an optional WASM decoder, and a remote-origin validator.
The browser page resolves `pangenome-range/reader` through an import map to the
built public ESM entry; it does not reach into reader source or substitute Node
file reads. A separate module origin serves page/reader/optional WASM assets,
while the archive origin records exact cross-origin `HEAD` and range traffic
with stable connection identifiers. Planned reader ranges stay separate from
requests observed at the origin so an HTTP-cache hit cannot be mistaken for a
library-cache hit.

## Range sources and traces

Both languages expose a 64-bit source length and exact offset/length reads.
Rust `FileRangeSource` uses positioned file reads, so it does not depend on or
mutate a shared seek cursor. TypeScript keeps offsets as `bigint` and implements
`HttpRangeSource`, `BlobRangeSource`, `MemoryRangeSource`,
`TracingRangeSource`, and the Node-only `FileRangeSource`.

Trace summaries retain raw call order, offsets, lengths, and success, then derive:

- request and successful-request counts;
- total, unique, and duplicate requested bytes;
- smallest and largest request;
- the number of disjoint ranges after overlap/adjacency coalescing;
- how many nonempty reads could be merged with another read.

Unique-byte accounting is the union of half-open requested intervals. Failed
reads are still requests and remain in the trace. Zero-length reads count as
operations but cover no bytes.

`HttpRangeSource` lazily discovers size with usable `HEAD` metadata and falls
back to `GET Range: bytes=0-0`. Exact reads require `206`, matching exposed
`Content-Range` and `Content-Length`, `Accept-Ranges: bytes`, and a stable
exposed `ETag`; later reads use `If-Range` by default. A `200` whole-object body
is rejected without reading it unless a caller explicitly configures a small
`maxFullResponseBytes` cap. Normal reads leave the browser HTTP cache enabled;
benchmarks opt into `cache: "no-store"` when measuring cold transport. The
integration origin also supplies CORS, immutable `no-transform` cache policy,
and raw request evidence. Browser/library and HTTP cache effects remain
reported separately.

The archive reader retains the bounded bootstrap/root, uses separate
byte-bounded LRU caches for directory pages, compressed graph payloads, and
extension descriptors/pages, fetches
missing pages as contiguous spans, and coalesces selected payloads into one
parallel dependency round. Optional query traces report exact ranges and layer
bytes, dependency rounds, cache hits, decode/decompression/merge timings,
selected counts, and the canonical BLAKE3 result. When tracing is disabled,
the merge remains required but trace accounting and hashing are skipped.

The default `named-loci-v1---` extension is a sorted fence descriptor plus
independently compressed leaves. It is empty unless an exact GFF3 input and
real reference sample binding are supplied. The default `summary-pyr-v1--`
extension is a fixed-grid, factor-four pyramid built from accepted core-tile
counters. Both are lazy range APIs and remain optional extension entries so an
unknown reader can still decode graph regions. Neither introduces sample
identity or merges anonymous traversals across tiles.

## Viewer pipeline

The `/viewer` entry is a one-way consumer of the reader API:

```text
PangenomeArchive.queryTiles()
  -> bounded incremental view model
  -> deterministic layout snapshot
  -> Canvas 2D renderer
  -> interaction/lifecycle controller
```

The viewer never opens URLs, reads byte ranges, parses archive bytes, or owns an
archive. Its controller cancels the previous `queryTiles()` iterator whenever a
new region is selected. Reference nodes are retained before alternatives when a
budget is reached; node, edge, and traversal caps are applied before layout so a
large decoded region cannot create an unbounded render corpus. The current
bounded main-thread layout did not justify worker-transfer complexity; a worker
is reserved for a measured interaction failure rather than assumed to help.

Reference topology can merge by node identity. Weighted anonymous traversals
remain attached to their source tile, are ranked by local multiplicity for the
limited frequency lanes, and are never stitched across tile boundaries. The
canvas shows reference coordinate ticks, orientation, alternate branches,
curved topology edges, tile boundaries, local weights, and explicit summarized
counts. Mouse, pointer, and keyboard controls share one transform; `destroy()`
aborts work, removes listeners and observers, and releases all DOM owned by the
viewer.

## Cost simulation

`NetworkProfile` estimates idealized latency as:

```text
ceil(requests / max_parallel) * (RTT + per-request overhead)
  + requested bits / bandwidth
```

The aggregate estimator is optimistic: it assumes all requests are known up
front and perfectly parallelizable. Candidate experiments use a second method
that accepts explicit dependency groups, so the bootstrap/index round precedes
the data round. Both models still exclude connection setup, congestion, server
variance, browser scheduling, decompression, parsing, and computation. They are
useful for ranking early layouts but cannot replace real HTTP/browser benchmarks.

## Correctness boundary

`CanonicalSubgraph` represents node IDs/sequences, oriented edge topology, the
real reference traversal, and reference intervals. `CanonicalHaplotypeTile`
separately represents anonymous local traversals, exact weights, and tile
provenance.
Maps and sets remove irrelevant node/edge ordering; path collections are sorted
while retaining duplicate multiplicity; traversal order and orientation remain
significant. A stable, domain-separated BLAKE3 digest makes oracle/candidate
comparisons cheap to store.

The experiment now emits `CanonicalSubgraph` from:

1. an upstream GBZ/GBZ-base reference query, and
2. the fixed-window candidate reader over `RangeSource`.

Every current-format query has two gates. The assembled graph is compared with an
independent local source extraction for node sequences, oriented edges, the
reference traversal, and coordinates. Every selected tile is then freshly
extracted from the source with the exact core interval and construction halo and
compared for oriented traversal, weight, total weight, semantics, and
provenance. Anonymous paths are never stitched across tiles. The oracle remains
a full local source load and is not presented as a remotely efficient reader.

## Upstream API boundary discovered

The `gbz` 0.7.0 crate safely exposes graph nodes, paths, metadata, tags,
translation presence, sample/contig names, haplotype count, and reference-sample
tags after full deserialization. The inspection CLI uses those APIs. It does not
claim the upstream load is range-efficient. The experiment oracle also uses
those APIs: `simple_sds::serialize::load_from` opens and deserializes the whole
GBZ. That makes it a source/oracle path, not the candidate remote reader.
