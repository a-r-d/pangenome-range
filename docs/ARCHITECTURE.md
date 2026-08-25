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

## TypeScript product boundary

The TypeScript workspace is a separate consumer of the static archive, not a
Rust crate or native-binary wrapper. `packages/browser` publishes isolated
reader, viewer, and Node entry points. The root and `/reader` entries contain no
DOM or Node built-ins; `/viewer` owns the framework-neutral rendering contract,
and `/node` owns Node-only range sources. `packages/benchmark` is private and
will own Node and real-browser measurements.

Only the public contracts and a Node file-range source exist in the current
scaffold. Archive decoding, HTTP range transport, rendering, and browser
benchmarks remain explicitly unimplemented.

## Range sources and traces

`RangeSource` exposes a 64-bit source length and exact reads by `(u64 offset,
usize length)`. `FileRangeSource` uses positioned file reads, so it does not
depend on or mutate a shared seek cursor. `TracingRangeSource<T>` is a generic
decorator and records all attempted reads, including failures.

Trace summaries retain raw call order, offsets, lengths, and success, then derive:

- request and successful-request counts;
- total, unique, and duplicate requested bytes;
- smallest and largest request;
- the number of disjoint ranges after overlap/adjacency coalescing;
- how many nonempty reads could be merged with another read.

Unique-byte accounting is the union of half-open requested intervals. Failed
reads are still requests and remain in the trace. Zero-length reads count as
operations but cover no bytes.

An eventual HTTP implementation should satisfy the same exact-read contract and
add transport evidence separately (status, `Content-Range`, response bytes,
cache state, and timing). It is intentionally not part of the bootstrap.

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
