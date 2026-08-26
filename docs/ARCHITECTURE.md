# Architecture

## Dependency direction

```text
pangenome-range-cli ──> pangenome-range-build ──> pangenome-range-query
     │                         │                         │
     └────> upstream gbz/gbz-base                       ▼
                               └────────> pangenome-range-format
```

`pangenome-range-format` contains the storage seam and network cost model, but no
graph semantics. `pangenome-range-query` contains the canonical semantic result
types. `pangenome-range-build` contains the GBZ/GBZ-base source adapters and the
first concrete candidate layout and its external-memory encoder. The current
Candidate 1 object is specified in
[Fixed-window archive v4](FIXED_WINDOW_ARCHIVE.md).

## Direct archive construction

The normal v4 encoder writes a temporary sibling of the requested `.pngr`
object. It emits the provisional 64-byte header and root, reserves the exact
fixed directory-page span, and then appends accepted payloads in deterministic
reference/coordinate order. Compact per-reference/per-bucket descriptors are
retained for directory backfill; there is no payload spool, second full-file
copy, occurrence table, or global pending-entry sort.

The normal regional path uses `HaplotypeOutput::None` only to select local graph
state. It copies the exact compressed GBWT records, forward node sequences, and
canonical topology edges; it does not enumerate or sort anonymous paths while
encoding. Exact borrowed record lengths provide the adaptive-split preflight,
so an oversized parent is rejected before a second payload corpus is copied.
The real reference is retained as a GBWT occurrence anchor with fragment and
node offsets. Weighted anonymous paths are reconstructed only by a reader for
the tiles selected by a query.

Tile selection/materialization and compression use bounded batches controlled
by `--threads` and `--max-queued-bytes`; the CLI defaults to available
parallelism capped at eight and a 256 MiB queue cap. Results are consumed in
input order, and adaptive children precede later completed work, so worker
completion order cannot change archive offsets or bytes. The completed
temporary object is flushed, every physical payload is decompressed and
structurally bounds-checked, and then it is atomically renamed. Failed temporary
objects are removed unless `--keep-partial` is set.

The encoder computes exact total selected reference bases before payload work.
Progress snapshots report current reference/coordinate, accepted and physical
chunks, processed/total bases, global and per-reference percentage, observed
bp/s and chunks/s, a rate-derived ETA, build/processing elapsed time, and the
current temp-final byte length. JSON and plain progress contain the same
measurements; `--progress-interval-seconds` controls their cadence. Percent is
coordinate-based rather than inferred from compressed bytes.

## TypeScript product boundary

The TypeScript workspace is a separate consumer of the static archive, not a
Rust crate or native-binary wrapper. `packages/browser` publishes isolated
reader, viewer, and Node entry points. The root and `/reader` entries contain no
DOM or Node built-ins; `/viewer` owns the framework-neutral rendering contract,
and `/node` owns Node-only range sources. `packages/benchmark` is private and
will own Node and real-browser measurements.

The public contracts, strict HTTP/Blob/memory/file range sources, archive-v4
bootstrap/root and arithmetic directory reader, byte-bounded caches,
pure-JavaScript zstd decompressor, and record-preserving regional payload
decoder exist. Rust and TypeScript decode the same raw golden payload into
typed-array-oriented nodes, sequences, topology, reference traversal, and
weighted local paths. Rendering and a public-network browser benchmark corpus
remain explicitly unimplemented.

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

`HttpRangeSource` requires exact `206`, matching exposed `Content-Range` and
`Content-Length`, `Accept-Ranges: bytes`, and a stable exposed `ETag`. It never
accepts `200` fallback. The integration origin also supplies CORS, immutable
`no-transform` cache policy, and raw request evidence. Browser/library and HTTP
cache effects remain reported separately.

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

Every retained v4 query has two gates. The assembled graph is compared with an
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
