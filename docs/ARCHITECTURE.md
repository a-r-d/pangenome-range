# Bootstrap architecture

## Dependency direction

```text
pangenome-range-cli ──> pangenome-range-query ──> pangenome-range-format
     │                                  ▲
     └────> upstream gbz                │
                                        │
pangenome-range-build ─────────────────────────────────────┘
```

`pangenome-range-format` contains the storage seam, but no graph semantics and no
archive specification. `pangenome-range-query` contains semantic result types and only
depends on the `RangeSource` interface. Candidate builders and the CLI can use
those lower layers without forcing query code to open a local file.

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

This optimistic model assumes all requests are known up front and perfectly
parallelizable. It excludes connection setup, dependency chains, congestion,
server variance, browser scheduling, decompression, parsing, and computation.
It is useful for ranking early layouts but cannot replace real HTTP/browser
benchmarks.

## Correctness boundary

`CanonicalSubgraph` represents node IDs/sequences, oriented edge topology,
named path/haplotype traversals, reference-path markers, and reference intervals.
Maps and sets remove irrelevant node/edge ordering; path collections are sorted
and deduplicated; traversal order and orientation remain significant. A stable,
domain-separated BLAKE3 digest makes oracle/candidate comparisons cheap to store.

The next correctness step is an adapter that emits `CanonicalSubgraph` from:

1. an upstream GBZ/GBZ-base reference query, and
2. each candidate reader over `RangeSource`.

The adapter is not fabricated in this bootstrap because the upstream Rust `gbz`
crate loads serialized GBZ from a path/reader and does not expose its internal
serialized structures through this project's `RangeSource`. The canonical model
and comparison behavior are implemented; query extraction remains the next
semantic integration.

## Upstream API boundary discovered

The `gbz` 0.7.0 crate safely exposes graph nodes, paths, metadata, tags,
translation presence, sample/contig names, haplotype count, and reference-sample
tags after full deserialization. The inspection CLI uses those APIs. It does not
claim the upstream load is range-efficient: `simple_sds::serialize::load_from`
opens and deserializes the whole GBZ. That makes it a source/oracle path, not the
candidate remote reader.
