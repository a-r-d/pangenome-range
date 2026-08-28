# Viewer performance

## Current production browser

The production `/demo` route now uses the deterministic, anchored SVG tube-map
renderer described in [ADR 0003](adr/0003-tube-map-renderer.md). Its current
built-browser evidence, bundle comparison, screenshots, and configured-archive
measurements are retained in
`results/2026-08-27-simple-browser-v1/REPORT.md`.

In the retained Chromium observation, HLA-B completed in 1,431.9 ms after
opening the configured 8.2 GiB immutable archive, with the first tile visible
at 1,254.6 ms. The renderer laid out 141 node groups, 240 topology edges, and
eight weighted tile-local patterns in 6.5 ms. MICB completed in 664.1 ms on the
already-open archive with 82 groups, 153 edges, and eight patterns. These are
single public-network observations, not latency distributions.

The remainder of this page preserves the earlier Canvas-explorer measurements
because they remain useful reader/decode evidence. They are not measurements of
the current production UI.

## Measurement model

Elapsed wall time and aggregate task work are different quantities. Reader
traces merge concurrent time intervals before reporting a wall-clock phase.
Integrity and regional-decode intervals are also merged, so concurrent tiles
are not added together and mislabeled as elapsed time.

`decompressionMs` is decompression critical-path wall time.
`decompressionTaskMs` is the sum of individual decompression task durations and
may exceed wall time for an asynchronous/parallel decompressor. The benchmark
wrapper preserves synchronous decoder returns; this prevents time waiting for
unrelated decode continuations from being attributed to decompression.

The retired explorer added object-open, first-summary-paint, first-tile-paint,
query-complete, viewer-model, layout, paint, and rolling frame-p95 evidence.
The range waterfall retains exact offsets, lengths, and logical layers. Browser
origin logs remain authoritative for actual request counts and bytes; reader
plans and cache hits are reported separately.

## Retained 100 kb detailed query

Workload: `CHM13#chr1:1,000,000-1,100,000`, context 100, Chromium, loopback
strict-range origin, 8,832,749,949-byte current-v1 archive SHA-256
`ecf5ae4fa8c784a80307507f58bed894311b8560724b57de0fcc35237c324b63`.
Every result matched canonical hash
`b191be02fc2a9556349d8b5b97b268c90c579b1c275cc600355bfaae5b499473`.

| Metric | Retained before | Current pure JS | Current WASM |
| --- | ---: | ---: | ---: |
| Cold no-store query | 1,678.3 ms | 1,565.3 ms | 1,458.1 ms |
| Cold total including open | 1,717.3 ms complete UI | 1,577.7 ms harness | 1,485.3 ms harness |
| Actual requests / bytes | 4 / 294,198 B | 4 / 294,358 B | 4 / 294,358 B |
| Decompression wall/task | previously overlapping | 32.2 / 32.2 ms | 6.5 / 6.5 ms |
| Integrity | not exclusive | 32.8 ms | 32.8 ms |
| Regional decode | 1,352.8 ms accumulated | 1,373.4 ms wall union | 1,288.5 ms wall union |
| Merge | not comparable | 55.5 ms | 63.4 ms |

The pure-JS cold query improved 6.7%, and the WASM observation improved 13.1%
against the retained query wall. Neither reaches the requested 2x target
(`<=839.2 ms`). The measurements show that zstd is no longer the governing
phase: WASM reduced decompression by 25.7 ms but reduced the observed query by
only 107.2 ms relative to pure JS in this run. Regional payload decode and GBWT
reconstruction dominate.

WASM adds 251,806 deployable bytes and measured 6.8 ms one-time initialization.
It therefore remains optional. Bounded decompression/reconstruction workers,
transferable-buffer reconstruction, and OffscreenCanvas were not adopted:
there is not yet a controlled implementation that demonstrates an
end-to-end or interaction win without extra copies and lifecycle complexity.

## Search, summaries, and public origin

One public-path observation after archive open measured exact `HLA-B` at
125.5 ms cold (69,112 bytes, two dependency rounds) and 1.2 ms warm (zero
bytes). A limited `HLA-` prefix returned five results with an explicit
truncation flag from cached pages. The 100 kb CHM13 summary measured 106.2 ms
cold (49,503 bytes, two rounds) and 0.3 ms warm. These public-network
observations are not relabeled as loopback percentiles.

The origin probe passed `HEAD`, exact and overlapping/tail `206` reads, byte
comparison with the checksum-matched local object, stable ETag, identity
encoding, immutable/no-transform caching, CORS, exposed headers, and preflight
for the GitHub Pages origin. It transferred only the probed ranges.

## Cache and browser qualifications

- Cold library/normal HTTP cache uses a fresh Playwright context; operating
  system caches are not controlled.
- Warm-library scenarios force transport `no-store` so library and browser HTTP
  caches are not conflated.
- The 100 kb comparison is loopback runtime evidence, not CDN latency.
- Public search/summary measurements are a single observation, not a p50/p95
  corpus.
- Heap evidence is Chromium's exposed JavaScript heap only and excludes native
  and WASM memory.
- Frame p95 is a bounded canvas/app sample; a full DevTools long-task and Core
  Web Vitals trace remains future evidence.

Raw requests, scenarios, phase measurements, environment, screenshots, and the
origin report are retained in
`results/2026-08-27-viewer-explorer-v1/`.
