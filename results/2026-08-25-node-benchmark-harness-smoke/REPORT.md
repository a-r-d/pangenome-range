# Node range benchmark: 2026-08-25-node-benchmark-harness-smoke

Status: **correctness gate passed**.

This report contains real positioned-file measurements. Planned reader ranges and dependency rounds are reported separately from requests actually observed at the transport/origin. No simulated Rust latency is relabeled as Node or browser performance.

## Archive and workload

- Location: `/media/ard/samsung-2tb/projects/pangenome-range/test-data/conformance/micb-kir3dl1-reader-v1.pngr`
- Size: 164259 bytes
- Archive SHA-256: 31f30622c44d71e54a07e0edc61f2ade3c1521b2f5a7092e56334a44b6c85565
- ETag: not applicable/unavailable
- Workload SHA-256: `1cf37b0053c20971c59b1c4f7349908a284eb1190dff633bf6b6235080558a98`

## Query measurements

| Runtime | Decoder | Scenario | Query | actual/planned requests | actual/planned bytes | actual/planned rounds | source/origin match | total ms | correct |
|---|---|---|---|---:|---:|---:|---:|---:|---|
| Node | pure-js | cold-library | fixed-micb | 1 / 2 | 21413 / 37797 | n/a / 2 | yes | 56.502 | yes |
| Node | pure-js | cold-library | fixed-kir3dl1 | 2 / 3 | 52231 / 68615 | n/a / 3 | yes | 106.665 | yes |
| Node | pure-js | cold-library | boundary-start | 1 / 2 | 14771 / 31155 | n/a / 2 | yes | 16.183 | yes |
| Node | pure-js | cold-library | pan-anchor | 1 / 2 | 21451 / 37835 | n/a / 2 | yes | 27.646 | yes |
| Node | pure-js | cold-library | nearby-pan | 1 / 2 | 21451 / 37835 | n/a / 2 | yes | 24.865 | yes |
| Node | pure-js | cold-library | boundary-end | 1 / 2 | 6680 / 23064 | n/a / 2 | yes | 6.172 | yes |
| Node | pure-js | cold-library | distant-random | 2 / 3 | 52231 / 68615 | n/a / 3 | yes | 94.277 | yes |
| Node | pure-js | cold-library | random-1000-00000 | 1 / 2 | 6388 / 22772 | n/a / 2 | yes | 5.869 | yes |
| Node | pure-js | cold-library | random-10000-00000 | 2 / 3 | 52231 / 68615 | n/a / 3 | yes | 83.293 | yes |
| Node | pure-js | cold-library | absent-reference | 0 / 0 | 0 / 0 | n/a / 0 | yes | 0.505 | yes |
| Node | pure-js | warm-repeated-query | fixed-micb | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 35.577 | yes |
| Node | pure-js | warm-repeated-query | fixed-kir3dl1 | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 94.180 | yes |
| Node | pure-js | warm-repeated-query | boundary-start | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 15.676 | yes |
| Node | pure-js | warm-repeated-query | pan-anchor | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 28.670 | yes |
| Node | pure-js | warm-repeated-query | nearby-pan | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 22.415 | yes |
| Node | pure-js | warm-repeated-query | boundary-end | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 5.358 | yes |
| Node | pure-js | warm-repeated-query | distant-random | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 80.151 | yes |
| Node | pure-js | warm-repeated-query | random-1000-00000 | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 11.848 | yes |
| Node | pure-js | warm-repeated-query | random-10000-00000 | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 89.285 | yes |
| Node | pure-js | warm-repeated-query | absent-reference | 0 / 0 | 0 / 0 | n/a / 0 | yes | 0.032 | yes |
| Node | wasm | cold-library | fixed-micb | 1 / 2 | 21413 / 37797 | n/a / 2 | yes | 24.763 | yes |
| Node | wasm | cold-library | fixed-kir3dl1 | 2 / 3 | 52231 / 68615 | n/a / 3 | yes | 87.812 | yes |
| Node | wasm | cold-library | boundary-start | 1 / 2 | 14771 / 31155 | n/a / 2 | yes | 14.417 | yes |
| Node | wasm | cold-library | pan-anchor | 1 / 2 | 21451 / 37835 | n/a / 2 | yes | 20.467 | yes |
| Node | wasm | cold-library | nearby-pan | 1 / 2 | 21451 / 37835 | n/a / 2 | yes | 20.622 | yes |
| Node | wasm | cold-library | boundary-end | 1 / 2 | 6680 / 23064 | n/a / 2 | yes | 6.372 | yes |
| Node | wasm | cold-library | distant-random | 2 / 3 | 52231 / 68615 | n/a / 3 | yes | 72.267 | yes |
| Node | wasm | cold-library | random-1000-00000 | 1 / 2 | 6388 / 22772 | n/a / 2 | yes | 5.031 | yes |
| Node | wasm | cold-library | random-10000-00000 | 2 / 3 | 52231 / 68615 | n/a / 3 | yes | 79.053 | yes |
| Node | wasm | cold-library | absent-reference | 0 / 0 | 0 / 0 | n/a / 0 | yes | 0.362 | yes |
| Node | wasm | warm-repeated-query | fixed-micb | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 36.106 | yes |
| Node | wasm | warm-repeated-query | fixed-kir3dl1 | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 81.913 | yes |
| Node | wasm | warm-repeated-query | boundary-start | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 13.074 | yes |
| Node | wasm | warm-repeated-query | pan-anchor | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 20.466 | yes |
| Node | wasm | warm-repeated-query | nearby-pan | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 20.461 | yes |
| Node | wasm | warm-repeated-query | boundary-end | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 4.460 | yes |
| Node | wasm | warm-repeated-query | distant-random | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 76.006 | yes |
| Node | wasm | warm-repeated-query | random-1000-00000 | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 4.427 | yes |
| Node | wasm | warm-repeated-query | random-10000-00000 | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 70.988 | yes |
| Node | wasm | warm-repeated-query | absent-reference | 0 / 0 | 0 / 0 | n/a / 0 | yes | 0.034 | yes |

Latency p50/p95/max: 20.622 / 94.180 / 106.665 ms across 40 measurements.

## Decoder comparison

| Decoder | init ms | JS bytes | WASM bytes | chunk p50 ms | chunk p95 ms | peak heap bytes |
|---|---:|---:|---:|---:|---:|---:|
| pure-js | 0.026 | 112864 | 0 | 2.717 | 8.477 | 205242120 |
| wasm | 1.462 | 137271 | 251806 | 0.206 | 0.651 | 285886240 |

The WASM decoder remains optional. Initialization, deployable asset bytes, per-chunk time, total query time, available memory evidence, and correctness all remain visible; steady-state decompression alone does not select the default.

## Limitations

- Positioned-file reads measure the reader and local storage path; they are not HTTP or browser measurements.
- Cold library mode creates a new archive reader per query; operating-system and remote CDN cache state are uncontrolled.
- Warm mode primes the exact query, retaining both directory and compressed-payload library caches within their configured byte budgets.
