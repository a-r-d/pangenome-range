# Node range benchmark: 2026-08-25-node-mhc-benchmark-smoke

Status: **correctness gate passed**.

This report contains real positioned-file measurements. Planned reader ranges and dependency rounds are reported separately from requests actually observed at the transport/origin. No simulated Rust latency is relabeled as Node or browser performance.

## Archive and workload

- Location: `/tmp/pangenome-range-mhc-benchmark-v1.pngr`
- Size: 4806677 bytes
- Archive SHA-256: ec71bdfff9e0ebdf5bbbac9dcb77b547ba334054b5980d17cceba1c29509e1c5
- ETag: not applicable/unavailable
- Workload SHA-256: `e25ce42427de983ffc4f41014ea9f9f97bdd9639d9789c4164054de6784a3240`

## Query measurements

| Runtime | Decoder | Scenario | Query | actual/planned requests | actual/planned bytes | actual/planned rounds | source/origin match | total ms | correct |
|---|---|---|---|---:|---:|---:|---:|---:|---|
| Node | pure-js | cold-library | boundary-start | 1 / 2 | 7715 / 24099 | n/a / 2 | yes | 12.180 | yes |
| Node | pure-js | cold-library | pan-anchor | 2 / 3 | 18549 / 34933 | n/a / 3 | yes | 17.284 | yes |
| Node | pure-js | cold-library | nearby-pan | 2 / 3 | 35482 / 51866 | n/a / 3 | yes | 17.418 | yes |
| Node | pure-js | cold-library | boundary-end | 2 / 3 | 8155 / 24539 | n/a / 3 | yes | 1.940 | yes |
| Node | pure-js | cold-library | distant-random | 2 / 3 | 18549 / 34933 | n/a / 3 | yes | 9.995 | yes |
| Node | pure-js | cold-library | random-1000-00000 | 2 / 3 | 11671 / 28055 | n/a / 3 | yes | 2.295 | yes |
| Node | pure-js | cold-library | random-1000-00001 | 2 / 3 | 14649 / 31033 | n/a / 3 | yes | 6.367 | yes |
| Node | pure-js | cold-library | random-10000-00000 | 2 / 3 | 17794 / 34178 | n/a / 3 | yes | 7.194 | yes |
| Node | pure-js | cold-library | random-10000-00001 | 2 / 3 | 22652 / 39036 | n/a / 3 | yes | 5.392 | yes |
| Node | pure-js | cold-library | random-100000-00000 | 2 / 3 | 70347 / 86731 | n/a / 3 | yes | 26.494 | yes |
| Node | pure-js | cold-library | random-100000-00001 | 2 / 3 | 126552 / 142936 | n/a / 3 | yes | 82.993 | yes |
| Node | pure-js | cold-library | random-1000000-00000 | 2 / 3 | 1699944 / 1716328 | n/a / 3 | yes | 1875.187 | yes |
| Node | pure-js | cold-library | random-1000000-00001 | 2 / 3 | 1081223 / 1097607 | n/a / 3 | yes | 884.035 | yes |
| Node | pure-js | cold-library | absent-reference | 0 / 0 | 0 / 0 | n/a / 0 | yes | 11.668 | yes |
| Node | pure-js | warm-repeated-query | boundary-start | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 1.555 | yes |
| Node | pure-js | warm-repeated-query | pan-anchor | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 6.922 | yes |
| Node | pure-js | warm-repeated-query | nearby-pan | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 12.957 | yes |
| Node | pure-js | warm-repeated-query | boundary-end | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 1.161 | yes |
| Node | pure-js | warm-repeated-query | distant-random | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 6.701 | yes |
| Node | pure-js | warm-repeated-query | random-1000-00000 | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 1.356 | yes |
| Node | pure-js | warm-repeated-query | random-1000-00001 | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 4.601 | yes |
| Node | pure-js | warm-repeated-query | random-10000-00000 | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 5.978 | yes |
| Node | pure-js | warm-repeated-query | random-10000-00001 | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 4.712 | yes |
| Node | pure-js | warm-repeated-query | random-100000-00000 | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 20.811 | yes |
| Node | pure-js | warm-repeated-query | random-100000-00001 | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 76.908 | yes |
| Node | pure-js | warm-repeated-query | random-1000000-00000 | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 1719.308 | yes |
| Node | pure-js | warm-repeated-query | random-1000000-00001 | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 805.956 | yes |
| Node | pure-js | warm-repeated-query | absent-reference | 0 / 0 | 0 / 0 | n/a / 0 | yes | 0.035 | yes |
| Node | wasm | cold-library | boundary-start | 1 / 2 | 7715 / 24099 | n/a / 2 | yes | 2.912 | yes |
| Node | wasm | cold-library | pan-anchor | 2 / 3 | 18549 / 34933 | n/a / 3 | yes | 6.005 | yes |
| Node | wasm | cold-library | nearby-pan | 2 / 3 | 35482 / 51866 | n/a / 3 | yes | 9.955 | yes |
| Node | wasm | cold-library | boundary-end | 2 / 3 | 8155 / 24539 | n/a / 3 | yes | 0.938 | yes |
| Node | wasm | cold-library | distant-random | 2 / 3 | 18549 / 34933 | n/a / 3 | yes | 4.958 | yes |
| Node | wasm | cold-library | random-1000-00000 | 2 / 3 | 11671 / 28055 | n/a / 3 | yes | 0.851 | yes |
| Node | wasm | cold-library | random-1000-00001 | 2 / 3 | 14649 / 31033 | n/a / 3 | yes | 2.826 | yes |
| Node | wasm | cold-library | random-10000-00000 | 2 / 3 | 17794 / 34178 | n/a / 3 | yes | 4.726 | yes |
| Node | wasm | cold-library | random-10000-00001 | 2 / 3 | 22652 / 39036 | n/a / 3 | yes | 3.032 | yes |
| Node | wasm | cold-library | random-100000-00000 | 2 / 3 | 70347 / 86731 | n/a / 3 | yes | 14.524 | yes |
| Node | wasm | cold-library | random-100000-00001 | 2 / 3 | 126552 / 142936 | n/a / 3 | yes | 65.523 | yes |
| Node | wasm | cold-library | random-1000000-00000 | 2 / 3 | 1699944 / 1716328 | n/a / 3 | yes | 1482.071 | yes |
| Node | wasm | cold-library | random-1000000-00001 | 2 / 3 | 1081223 / 1097607 | n/a / 3 | yes | 711.085 | yes |
| Node | wasm | cold-library | absent-reference | 0 / 0 | 0 / 0 | n/a / 0 | yes | 0.411 | yes |
| Node | wasm | warm-repeated-query | boundary-start | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 0.911 | yes |
| Node | wasm | warm-repeated-query | pan-anchor | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 7.015 | yes |
| Node | wasm | warm-repeated-query | nearby-pan | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 8.996 | yes |
| Node | wasm | warm-repeated-query | boundary-end | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 0.704 | yes |
| Node | wasm | warm-repeated-query | distant-random | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 4.863 | yes |
| Node | wasm | warm-repeated-query | random-1000-00000 | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 0.616 | yes |
| Node | wasm | warm-repeated-query | random-1000-00001 | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 2.911 | yes |
| Node | wasm | warm-repeated-query | random-10000-00000 | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 4.256 | yes |
| Node | wasm | warm-repeated-query | random-10000-00001 | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 2.745 | yes |
| Node | wasm | warm-repeated-query | random-100000-00000 | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 14.396 | yes |
| Node | wasm | warm-repeated-query | random-100000-00001 | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 65.185 | yes |
| Node | wasm | warm-repeated-query | random-1000000-00000 | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 1580.007 | yes |
| Node | wasm | warm-repeated-query | random-1000000-00001 | 0 / 1 | 0 / 16384 | n/a / 1 | yes | 670.579 | yes |
| Node | wasm | warm-repeated-query | absent-reference | 0 / 0 | 0 / 0 | n/a / 0 | yes | 0.032 | yes |

Latency p50/p95/max: 6.367 / 1580.007 / 1875.187 ms across 56 measurements.

## Decoder comparison

| Decoder | init ms | JS bytes | WASM bytes | chunk p50 ms | chunk p95 ms | peak heap bytes |
|---|---:|---:|---:|---:|---:|---:|
| pure-js | 0.036 | 112864 | 0 | 37.844 | 243.481 | 463861656 |
| wasm | 1.559 | 137271 | 251806 | 2.429 | 12.359 | 287912536 |

The WASM decoder remains optional. Initialization, deployable asset bytes, per-chunk time, total query time, available memory evidence, and correctness all remain visible; steady-state decompression alone does not select the default.

## Limitations

- Positioned-file reads measure the reader and local storage path; they are not HTTP or browser measurements.
- Cold library mode creates a new archive reader per query; operating-system and remote CDN cache state are uncontrolled.
- Warm mode primes the exact query, retaining both directory and compressed-payload library caches within their configured byte budgets.
