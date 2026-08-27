# Real-browser range benchmark: viewer-explorer-final-corrected

Status: **correctness gate passed**.

This report contains real HTTP measurements. Planned reader ranges and dependency rounds are reported separately from requests actually observed at the transport/origin. No simulated Rust latency is relabeled as Node or browser performance.

## Archive and workload

- Location: `/media/ard/eba76579-d702-4ff0-b5dd-eb503a726a4d/pangenome-range-data/runs/2026-08-26-gencode-v50-whole-hprc/hprc-v1-gencode-v50-disk-t8.pngr`
- Size: 8832749949 bytes
- Archive SHA-256: ecf5ae4fa8c784a80307507f58bed894311b8560724b57de0fcc35237c324b63
- ETag: "sha256-ecf5ae4fa8c784a80307507f58bed894311b8560724b57de0fcc35237c324b63"
- Workload SHA-256: `ebc9c65f44299780dfb19ee4ea11d2674dbdefd3b608a2f82a46628642791463`

## Query measurements

| Runtime | Decoder | Scenario | Query | actual/planned requests | actual/planned bytes | actual/planned rounds | source/origin match | total ms | correct |
|---|---|---|---|---:|---:|---:|---:|---:|---|
| chromium | pure-js | cold-library-transport-no-store | chm13-chr1-100k | 4 / 4 | 294358 / 294358 | 4 / 4 | yes | 1577.700 | yes |
| chromium | pure-js | cold-library-normal-http-cache | chm13-chr1-100k | 4 / 4 | 294358 / 294358 | 4 / 4 | yes | 1451.200 | yes |
| chromium | pure-js | warm-directory-cache | chm13-chr1-100k | 1 / 3 | 255053 / 286166 | 1 / 3 | yes | 1482.200 | yes |
| chromium | pure-js | repeated-same-query | chm13-chr1-100k | 0 / 2 | 0 / 31113 | 0 / 2 | yes | 1374.300 | yes |
| chromium | pure-js | nearby-pan-query | chm13-chr1-100k | 0 / 2 | 0 / 31113 | 0 / 2 | yes | 1526.800 | yes |
| chromium | pure-js | distant-random-query | chm13-chr1-100k | 0 / 2 | 0 / 31113 | 0 / 2 | yes | 1341.400 | yes |
| chromium | wasm | cold-library-transport-no-store | chm13-chr1-100k | 4 / 4 | 294358 / 294358 | 4 / 4 | yes | 1485.300 | yes |
| chromium | wasm | cold-library-normal-http-cache | chm13-chr1-100k | 4 / 4 | 294358 / 294358 | 4 / 4 | yes | 1437.600 | yes |
| chromium | wasm | warm-directory-cache | chm13-chr1-100k | 1 / 3 | 255053 / 286166 | 1 / 3 | yes | 1399.800 | yes |
| chromium | wasm | repeated-same-query | chm13-chr1-100k | 0 / 2 | 0 / 31113 | 0 / 2 | yes | 1225.400 | yes |
| chromium | wasm | nearby-pan-query | chm13-chr1-100k | 0 / 2 | 0 / 31113 | 0 / 2 | yes | 1206.700 | yes |
| chromium | wasm | distant-random-query | chm13-chr1-100k | 0 / 2 | 0 / 31113 | 0 / 2 | yes | 1179.500 | yes |

Latency p50/p95/max: 1399.800 / 1577.700 / 1577.700 ms across 12 measurements.

## Decoder comparison

| Decoder | init ms | JS bytes | WASM bytes | chunk p50 ms | chunk p95 ms | peak heap bytes |
|---|---:|---:|---:|---:|---:|---:|
| pure-js | 0.100 | 166860 | 0 | 3.200 | 8.300 | 10000000 |
| wasm | 6.800 | 185356 | 251806 | 0.400 | 1.200 | 10000000 |

The WASM decoder remains optional. Initialization, deployable asset bytes, per-chunk time, total query time, available memory evidence, and correctness all remain visible; steady-state decompression alone does not select the default.

## Limitations

- All retained browser timings use a loopback origin and are functional/local evidence, not public-network or CDN performance.
- Cold HTTP-cache scenarios use a fresh ephemeral Playwright browser context. This establishes an empty context cache, but does not claim control over operating-system caches.
- Warm library-cache scenarios force transport no-store so directory and payload cache effects are not confused with the browser HTTP cache.
- Actual request counts/bytes/rounds come from range-origin logs; reader-observed fetches are retained and must reconcile with the origin. Planned counts/bytes and phase timings come from the reader query trace and Performance API.
- Peak JavaScript heap is available only where the browser exposes performance.memory and excludes native/WASM memory.
