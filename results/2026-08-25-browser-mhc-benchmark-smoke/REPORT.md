# Real-browser range benchmark: 2026-08-25-browser-mhc-benchmark-smoke

Status: **correctness gate passed**.

This report contains real HTTP measurements. Planned reader ranges and dependency rounds are reported separately from requests actually observed at the transport/origin. No simulated Rust latency is relabeled as Node or browser performance.

## Archive and workload

- Location: `/tmp/pangenome-range-mhc-benchmark-v1.pngr`
- Size: 4806677 bytes
- Archive SHA-256: ec71bdfff9e0ebdf5bbbac9dcb77b547ba334054b5980d17cceba1c29509e1c5
- ETag: "sha256-ec71bdfff9e0ebdf5bbbac9dcb77b547ba334054b5980d17cceba1c29509e1c5"
- Workload SHA-256: `e25ce42427de983ffc4f41014ea9f9f97bdd9639d9789c4164054de6784a3240`

## Query measurements

| Runtime | Decoder | Scenario | Query | actual/planned requests | actual/planned bytes | actual/planned rounds | source/origin match | total ms | correct |
|---|---|---|---|---:|---:|---:|---:|---:|---|
| chromium | pure-js | cold-library-transport-no-store | boundary-start | 2 / 2 | 24099 / 24099 | 2 / 2 | yes | 24.500 | yes |
| chromium | pure-js | cold-library-normal-http-cache | boundary-start | 2 / 2 | 24099 / 24099 | 2 / 2 | yes | 20.000 | yes |
| chromium | pure-js | warm-directory-cache | boundary-start | 1 / 2 | 7715 / 24099 | 1 / 2 | yes | 7.200 | yes |
| chromium | pure-js | repeated-same-query | boundary-start | 0 / 1 | 0 / 16384 | 0 / 1 | yes | 2.100 | yes |
| chromium | pure-js | nearby-pan-query | nearby-pan | 2 / 3 | 35482 / 51866 | 2 / 3 | yes | 25.500 | yes |
| chromium | pure-js | distant-random-query | distant-random | 0 / 1 | 0 / 16384 | 0 / 1 | yes | 9.200 | yes |
| chromium | wasm | cold-library-transport-no-store | boundary-start | 2 / 2 | 24099 / 24099 | 2 / 2 | yes | 27.000 | yes |
| chromium | wasm | cold-library-normal-http-cache | boundary-start | 2 / 2 | 24099 / 24099 | 2 / 2 | yes | 26.600 | yes |
| chromium | wasm | warm-directory-cache | boundary-start | 1 / 2 | 7715 / 24099 | 1 / 2 | yes | 5.100 | yes |
| chromium | wasm | repeated-same-query | boundary-start | 0 / 1 | 0 / 16384 | 0 / 1 | yes | 1.400 | yes |
| chromium | wasm | nearby-pan-query | nearby-pan | 2 / 3 | 35482 / 51866 | 2 / 3 | yes | 22.400 | yes |
| chromium | wasm | distant-random-query | distant-random | 0 / 1 | 0 / 16384 | 0 / 1 | yes | 7.900 | yes |
| firefox | pure-js | cold-library-transport-no-store | boundary-start | 2 / 2 | 24099 / 24099 | 2 / 2 | yes | 99.000 | yes |
| firefox | pure-js | cold-library-normal-http-cache | boundary-start | 2 / 2 | 24099 / 24099 | 2 / 2 | yes | 26.000 | yes |
| firefox | pure-js | warm-directory-cache | boundary-start | 1 / 2 | 7715 / 24099 | 1 / 2 | yes | 11.000 | yes |
| firefox | pure-js | repeated-same-query | boundary-start | 0 / 1 | 0 / 16384 | 0 / 1 | yes | 4.000 | yes |
| firefox | pure-js | nearby-pan-query | nearby-pan | 2 / 3 | 35482 / 51866 | 2 / 3 | yes | 33.000 | yes |
| firefox | pure-js | distant-random-query | distant-random | 0 / 1 | 0 / 16384 | 0 / 1 | yes | 8.000 | yes |
| firefox | wasm | cold-library-transport-no-store | boundary-start | 2 / 2 | 24099 / 24099 | 2 / 2 | yes | 24.000 | yes |
| firefox | wasm | cold-library-normal-http-cache | boundary-start | 2 / 2 | 24099 / 24099 | 2 / 2 | yes | 24.000 | yes |
| firefox | wasm | warm-directory-cache | boundary-start | 1 / 2 | 7715 / 24099 | 1 / 2 | yes | 6.000 | yes |
| firefox | wasm | repeated-same-query | boundary-start | 0 / 1 | 0 / 16384 | 0 / 1 | yes | 3.000 | yes |
| firefox | wasm | nearby-pan-query | nearby-pan | 2 / 3 | 35482 / 51866 | 2 / 3 | yes | 23.000 | yes |
| firefox | wasm | distant-random-query | distant-random | 0 / 1 | 0 / 16384 | 0 / 1 | yes | 7.000 | yes |
| webkit | pure-js | cold-library-transport-no-store | boundary-start | 2 / 2 | 24099 / 24099 | 2 / 2 | yes | 21.000 | yes |
| webkit | pure-js | cold-library-normal-http-cache | boundary-start | 2 / 2 | 24099 / 24099 | 2 / 2 | yes | 20.000 | yes |
| webkit | pure-js | warm-directory-cache | boundary-start | 1 / 2 | 7715 / 24099 | 1 / 2 | yes | 6.000 | yes |
| webkit | pure-js | repeated-same-query | boundary-start | 0 / 1 | 0 / 16384 | 0 / 1 | yes | 3.000 | yes |
| webkit | pure-js | nearby-pan-query | nearby-pan | 2 / 3 | 35482 / 51866 | 2 / 3 | yes | 27.000 | yes |
| webkit | pure-js | distant-random-query | distant-random | 0 / 1 | 0 / 16384 | 0 / 1 | yes | 11.000 | yes |
| webkit | wasm | cold-library-transport-no-store | boundary-start | 2 / 2 | 24099 / 24099 | 2 / 2 | yes | 24.000 | yes |
| webkit | wasm | cold-library-normal-http-cache | boundary-start | 2 / 2 | 24099 / 24099 | 2 / 2 | yes | 24.000 | yes |
| webkit | wasm | warm-directory-cache | boundary-start | 1 / 2 | 7715 / 24099 | 1 / 2 | yes | 4.000 | yes |
| webkit | wasm | repeated-same-query | boundary-start | 0 / 1 | 0 / 16384 | 0 / 1 | yes | 2.000 | yes |
| webkit | wasm | nearby-pan-query | nearby-pan | 2 / 3 | 35482 / 51866 | 2 / 3 | yes | 23.000 | yes |
| webkit | wasm | distant-random-query | distant-random | 0 / 1 | 0 / 16384 | 0 / 1 | yes | 7.000 | yes |

Latency p50/p95/max: 11.000 / 33.000 / 99.000 ms across 36 measurements.

## Decoder comparison

| Decoder | init ms | JS bytes | WASM bytes | chunk p50 ms | chunk p95 ms | peak heap bytes |
|---|---:|---:|---:|---:|---:|---:|
| pure-js | 0.000 | 117926 | 0 | 2.900 | 5.000 | 10000000 |
| wasm | 6.000 | 136422 | 251806 | 1.000 | 2.000 | 10000000 |

The WASM decoder remains optional. Initialization, deployable asset bytes, per-chunk time, total query time, available memory evidence, and correctness all remain visible; steady-state decompression alone does not select the default.

## Limitations

- All retained browser timings use a loopback origin and are functional/local evidence, not public-network or CDN performance.
- Cold HTTP-cache scenarios use a fresh ephemeral Playwright browser context. This establishes an empty context cache, but does not claim control over operating-system caches.
- Warm library-cache scenarios force transport no-store so directory and payload cache effects are not confused with the browser HTTP cache.
- Actual request counts/bytes/rounds come from range-origin logs; reader-observed fetches are retained and must reconcile with the origin. Planned counts/bytes and phase timings come from the reader query trace and Performance API.
- Peak JavaScript heap is available only where the browser exposes performance.memory and excludes native/WASM memory.
