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
- **Browser benchmark:** a future real browser run with HTTP evidence. Never use
  the simulator as a substitute.

Report median, p95, maximum, sample count, and failures for a corpus; keep raw
per-query results under `results/` in a documented machine-readable form once a
candidate exists.

## Correctness gate

A performance result is invalid unless the candidate's `CanonicalSubgraph`
matches the oracle for node IDs/sequences, oriented edges, path traversals,
reference-path identity, and reference coordinates. Record both hashes and a
useful semantic diff on mismatch.

## Baselines

At minimum compare source GBZ full load/query, GBZ-base on local SSD, and every
candidate over local positioned reads. When HTTP support exists, serve immutable
objects from a range-capable origin and distinguish cold CDN, warm CDN, browser
cache, and origin responses.

## Experiment modes

The full comparative matrix remains:

```bash
cargo run --release -p pangenome-range-cli -- \
  benchmark-fixed-windows <graph.gbz> <run-id> [random-queries-per-size]
```

Before paying for all 20 window/compression builds on a new input, run the
current small-query candidate alone: 16 KiB base windows, zstd-3, the v3
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
