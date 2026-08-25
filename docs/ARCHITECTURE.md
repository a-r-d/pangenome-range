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
first concrete candidate layout. The implemented Candidate 0 object is specified
in [Fixed-window archive v1](FIXED_WINDOW_ARCHIVE.md).

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

`CanonicalSubgraph` represents node IDs/sequences, oriented edge topology,
named path/haplotype traversals, reference-path markers, and reference intervals.
Maps and sets remove irrelevant node/edge ordering; path collections are sorted
while retaining duplicate multiplicity; traversal order and orientation remain
significant. A stable, domain-separated BLAKE3 digest makes oracle/candidate
comparisons cheap to store.

The experiment now emits `CanonicalSubgraph` from:

1. an upstream GBZ/GBZ-base reference query, and
2. the fixed-window candidate reader over `RangeSource`.

Every retained Candidate 0 query is compared against that source oracle for node
sequences, oriented edges, path traversal multiplicity/orientation, and reference
coordinates. The candidate path is range-shaped; the oracle remains a full local
source load and is not presented as a remotely efficient reader.

## Upstream API boundary discovered

The `gbz` 0.7.0 crate safely exposes graph nodes, paths, metadata, tags,
translation presence, sample/contig names, haplotype count, and reference-sample
tags after full deserialization. The inspection CLI uses those APIs. It does not
claim the upstream load is range-efficient. The experiment oracle also uses
those APIs: `simple_sds::serialize::load_from` opens and deserializes the whole
GBZ. That makes it a source/oracle path, not the candidate remote reader.
