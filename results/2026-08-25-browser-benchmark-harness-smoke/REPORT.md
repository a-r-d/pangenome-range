# Real-browser range benchmark: 2026-08-25-browser-benchmark-harness-smoke

Status: **correctness gate passed**.

This report contains real HTTP measurements. Planned reader ranges and dependency rounds are reported separately from requests actually observed at the transport/origin. No simulated Rust latency is relabeled as Node or browser performance.

## Archive and workload

- Location: `/media/ard/samsung-2tb/projects/pangenome-range/test-data/conformance/micb-kir3dl1-reader-v1.pngr`
- Size: 164259 bytes
- Archive SHA-256: 31f30622c44d71e54a07e0edc61f2ade3c1521b2f5a7092e56334a44b6c85565
- ETag: "sha256-31f30622c44d71e54a07e0edc61f2ade3c1521b2f5a7092e56334a44b6c85565"
- Workload SHA-256: `1cf37b0053c20971c59b1c4f7349908a284eb1190dff633bf6b6235080558a98`

## Query measurements

| Runtime | Decoder | Scenario | Query | actual/planned requests | actual/planned bytes | actual/planned rounds | source/origin match | total ms | correct |
|---|---|---|---|---:|---:|---:|---:|---:|---|
| chromium | pure-js | cold-library-transport-no-store | fixed-micb | 2 / 2 | 37797 / 37797 | 2 / 2 | yes | 70.300 | yes |
| chromium | pure-js | cold-library-normal-http-cache | fixed-micb | 2 / 2 | 37797 / 37797 | 2 / 2 | yes | 66.400 | yes |
| chromium | pure-js | warm-directory-cache | fixed-micb | 1 / 2 | 21413 / 37797 | 1 / 2 | yes | 33.600 | yes |
| chromium | pure-js | repeated-same-query | fixed-micb | 0 / 1 | 0 / 16384 | 0 / 1 | yes | 29.700 | yes |
| chromium | pure-js | nearby-pan-query | nearby-pan | 1 / 2 | 21451 / 37835 | 1 / 2 | yes | 26.400 | yes |
| chromium | pure-js | distant-random-query | distant-random | 2 / 3 | 52231 / 68615 | 2 / 3 | yes | 80.700 | yes |
| chromium | wasm | cold-library-transport-no-store | fixed-micb | 2 / 2 | 37797 / 37797 | 2 / 2 | yes | 76.900 | yes |
| chromium | wasm | cold-library-normal-http-cache | fixed-micb | 2 / 2 | 37797 / 37797 | 2 / 2 | yes | 79.500 | yes |
| chromium | wasm | warm-directory-cache | fixed-micb | 1 / 2 | 21413 / 37797 | 1 / 2 | yes | 31.400 | yes |
| chromium | wasm | repeated-same-query | fixed-micb | 0 / 1 | 0 / 16384 | 0 / 1 | yes | 29.600 | yes |
| chromium | wasm | nearby-pan-query | nearby-pan | 1 / 2 | 21451 / 37835 | 1 / 2 | yes | 25.700 | yes |
| chromium | wasm | distant-random-query | distant-random | 2 / 3 | 52231 / 68615 | 2 / 3 | yes | 70.700 | yes |
| firefox | pure-js | cold-library-transport-no-store | fixed-micb | 2 / 2 | 37797 / 37797 | 2 / 2 | yes | 179.000 | yes |
| firefox | pure-js | cold-library-normal-http-cache | fixed-micb | 2 / 2 | 37797 / 37797 | 2 / 2 | yes | 74.000 | yes |
| firefox | pure-js | warm-directory-cache | fixed-micb | 1 / 2 | 21413 / 37797 | 1 / 2 | yes | 34.000 | yes |
| firefox | pure-js | repeated-same-query | fixed-micb | 0 / 1 | 0 / 16384 | 0 / 1 | yes | 31.000 | yes |
| firefox | pure-js | nearby-pan-query | nearby-pan | 1 / 2 | 21451 / 37835 | 1 / 2 | yes | 34.000 | yes |
| firefox | pure-js | distant-random-query | distant-random | 2 / 3 | 52231 / 68615 | 2 / 3 | yes | 119.000 | yes |
| firefox | wasm | cold-library-transport-no-store | fixed-micb | 2 / 2 | 37797 / 37797 | 2 / 2 | yes | 74.000 | yes |
| firefox | wasm | cold-library-normal-http-cache | fixed-micb | 2 / 2 | 37797 / 37797 | 2 / 2 | yes | 64.000 | yes |
| firefox | wasm | warm-directory-cache | fixed-micb | 1 / 2 | 21413 / 37797 | 1 / 2 | yes | 33.000 | yes |
| firefox | wasm | repeated-same-query | fixed-micb | 0 / 1 | 0 / 16384 | 0 / 1 | yes | 27.000 | yes |
| firefox | wasm | nearby-pan-query | nearby-pan | 1 / 2 | 21451 / 37835 | 1 / 2 | yes | 31.000 | yes |
| firefox | wasm | distant-random-query | distant-random | 2 / 3 | 52231 / 68615 | 2 / 3 | yes | 84.000 | yes |
| webkit | pure-js | cold-library-transport-no-store | fixed-micb | 2 / 2 | 37797 / 37797 | 2 / 2 | yes | 64.000 | yes |
| webkit | pure-js | cold-library-normal-http-cache | fixed-micb | 2 / 2 | 37797 / 37797 | 2 / 2 | yes | 63.000 | yes |
| webkit | pure-js | warm-directory-cache | fixed-micb | 1 / 2 | 21413 / 37797 | 1 / 2 | yes | 46.000 | yes |
| webkit | pure-js | repeated-same-query | fixed-micb | 0 / 1 | 0 / 16384 | 0 / 1 | yes | 30.000 | yes |
| webkit | pure-js | nearby-pan-query | nearby-pan | 1 / 2 | 21451 / 37835 | 1 / 2 | yes | 28.000 | yes |
| webkit | pure-js | distant-random-query | distant-random | 2 / 3 | 52231 / 68615 | 2 / 3 | yes | 101.000 | yes |
| webkit | wasm | cold-library-transport-no-store | fixed-micb | 2 / 2 | 37797 / 37797 | 2 / 2 | yes | 69.000 | yes |
| webkit | wasm | cold-library-normal-http-cache | fixed-micb | 2 / 2 | 37797 / 37797 | 2 / 2 | yes | 68.000 | yes |
| webkit | wasm | warm-directory-cache | fixed-micb | 1 / 2 | 21413 / 37797 | 1 / 2 | yes | 28.000 | yes |
| webkit | wasm | repeated-same-query | fixed-micb | 0 / 1 | 0 / 16384 | 0 / 1 | yes | 25.000 | yes |
| webkit | wasm | nearby-pan-query | nearby-pan | 1 / 2 | 21451 / 37835 | 1 / 2 | yes | 24.000 | yes |
| webkit | wasm | distant-random-query | distant-random | 2 / 3 | 52231 / 68615 | 2 / 3 | yes | 96.000 | yes |

Latency p50/p95/max: 46.000 / 119.000 / 179.000 ms across 36 measurements.

## Decoder comparison

| Decoder | init ms | JS bytes | WASM bytes | chunk p50 ms | chunk p95 ms | peak heap bytes |
|---|---:|---:|---:|---:|---:|---:|
| pure-js | 0.000 | 117926 | 0 | 2.300 | 13.000 | 10000000 |
| wasm | 8.000 | 136422 | 251806 | 1.000 | 3.000 | 10000000 |

The WASM decoder remains optional. Initialization, deployable asset bytes, per-chunk time, total query time, available memory evidence, and correctness all remain visible; steady-state decompression alone does not select the default.

## Limitations

- All retained browser timings use a loopback origin and are functional/local evidence, not public-network or CDN performance.
- Cold HTTP-cache scenarios use a fresh ephemeral Playwright browser context. This establishes an empty context cache, but does not claim control over operating-system caches.
- Warm library-cache scenarios force transport no-store so directory and payload cache effects are not confused with the browser HTTP cache.
- Actual request counts/bytes/rounds come from range-origin logs; reader-observed fetches are retained and must reconcile with the origin. Planned counts/bytes and phase timings come from the reader query trace and Performance API.
- Peak JavaScript heap is available only where the browser exposes performance.memory and excludes native/WASM memory.
