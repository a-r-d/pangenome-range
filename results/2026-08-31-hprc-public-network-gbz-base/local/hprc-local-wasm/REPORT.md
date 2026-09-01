# Node range benchmark: hprc-local-wasm

Status: **correctness gate passed**.

This report contains real positioned-file measurements. Planned reader ranges and dependency rounds are reported separately from requests actually observed at the transport/origin. No simulated Rust latency is relabeled as Node or browser performance.

## Archive and workload

- Location: `/media/ard/eba76579-d702-4ff0-b5dd-eb503a726a4d/pangenome-range-data/runs/2026-08-30-hprc-v2.1-named-membership/hprc-v2.1-gencode-v50-named-membership-82585cb612effbf4.pngr`
- Size: 10836425558 bytes
- Archive SHA-256: 82585cb612effbf414b1c8f38b049bc415876866168ccc929f9a885f06d97b0a
- ETag: not applicable/unavailable
- Workload SHA-256: `c2a88ba48b0221ed69fc8d7c09c221dedacacf4f2d3a40d44e0faf682aacfaba`

## Query measurements

| Runtime | Decoder | Scenario | Query | actual/planned requests | actual/planned bytes | actual/planned rounds | source/origin match | total ms | correct |
|---|---|---|---|---:|---:|---:|---:|---:|---|
| Node | wasm | cold-library | fixed-micb | 2 / 4 | 71553 / 102794 | n/a / 4 | yes | 442.060 | yes |
| Node | wasm | cold-library | fixed-kir3dl1 | 2 / 4 | 127912 / 159153 | n/a / 4 | yes | 957.979 | yes |
| Node | wasm | cold-library | boundary-start | 2 / 4 | 15314 / 46555 | n/a / 4 | yes | 62.169 | yes |
| Node | wasm | cold-library | pan-anchor | 2 / 4 | 56552 / 87793 | n/a / 4 | yes | 244.773 | yes |
| Node | wasm | cold-library | nearby-pan | 2 / 4 | 28389 / 59630 | n/a / 4 | yes | 110.223 | yes |
| Node | wasm | cold-library | boundary-end | 2 / 4 | 17802 / 49043 | n/a / 4 | yes | 7.613 | yes |
| Node | wasm | cold-library | distant-random | 2 / 4 | 4382 / 35623 | n/a / 4 | yes | 1.728 | yes |
| Node | wasm | cold-library | random-1000-00000 | 2 / 4 | 180102 / 211343 | n/a / 4 | yes | 4315.760 | yes |
| Node | wasm | cold-library | random-1000-00001 | 2 / 4 | 4675 / 35916 | n/a / 4 | yes | 1.488 | yes |
| Node | wasm | cold-library | random-1000-00002 | 2 / 4 | 25847 / 57088 | n/a / 4 | yes | 109.943 | yes |
| Node | wasm | cold-library | random-1000-00003 | 2 / 4 | 40880 / 72121 | n/a / 4 | yes | 287.523 | yes |
| Node | wasm | cold-library | random-1000-00004 | 2 / 4 | 9818 / 41059 | n/a / 4 | yes | 1.718 | yes |
| Node | wasm | cold-library | random-1000-00005 | 2 / 4 | 402128 / 433369 | n/a / 4 | yes | 6433.983 | yes |
| Node | wasm | cold-library | random-1000-00006 | 2 / 4 | 7800 / 39041 | n/a / 4 | yes | 1.562 | yes |
| Node | wasm | cold-library | random-1000-00007 | 2 / 4 | 4649 / 35890 | n/a / 4 | yes | 1.072 | yes |
| Node | wasm | cold-library | random-1000-00008 | 2 / 4 | 4691 / 35932 | n/a / 4 | yes | 0.938 | yes |
| Node | wasm | cold-library | random-1000-00009 | 2 / 4 | 27474 / 58715 | n/a / 4 | yes | 84.736 | yes |
| Node | wasm | cold-library | random-10000-00000 | 2 / 4 | 19313 / 50554 | n/a / 4 | yes | 3.729 | yes |
| Node | wasm | cold-library | random-10000-00001 | 2 / 4 | 58878 / 90119 | n/a / 4 | yes | 276.192 | yes |
| Node | wasm | cold-library | random-10000-00002 | 2 / 4 | 31420 / 62661 | n/a / 4 | yes | 15.933 | yes |
| Node | wasm | cold-library | random-10000-00003 | 2 / 4 | 46117 / 77358 | n/a / 4 | yes | 119.379 | yes |
| Node | wasm | cold-library | random-10000-00004 | 2 / 4 | 46386 / 77627 | n/a / 4 | yes | 150.260 | yes |
| Node | wasm | cold-library | random-10000-00005 | 2 / 4 | 66191 / 97432 | n/a / 4 | yes | 314.350 | yes |
| Node | wasm | cold-library | random-10000-00006 | 2 / 4 | 32642 / 63883 | n/a / 4 | yes | 140.887 | yes |
| Node | wasm | cold-library | random-10000-00007 | 2 / 4 | 30543 / 61784 | n/a / 4 | yes | 122.043 | yes |
| Node | wasm | cold-library | random-10000-00008 | 2 / 4 | 56901 / 88142 | n/a / 4 | yes | 207.674 | yes |
| Node | wasm | cold-library | random-10000-00009 | 2 / 4 | 34250 / 65491 | n/a / 4 | yes | 72.089 | yes |
| Node | wasm | cold-library | random-100000-00000 | 2 / 4 | 41729 / 72970 | n/a / 4 | yes | 6.058 | yes |
| Node | wasm | cold-library | random-100000-00001 | 2 / 4 | 481072 / 512313 | n/a / 4 | yes | 2861.753 | yes |
| Node | wasm | cold-library | random-100000-00002 | 2 / 4 | 66921 / 98162 | n/a / 4 | yes | 32.834 | yes |
| Node | wasm | cold-library | random-100000-00003 | 2 / 4 | 36506 / 67747 | n/a / 4 | yes | 10.185 | yes |
| Node | wasm | cold-library | random-100000-00004 | 2 / 4 | 196383 / 227624 | n/a / 4 | yes | 860.013 | yes |
| Node | wasm | cold-library | random-100000-00005 | 2 / 4 | 13331 / 44572 | n/a / 4 | yes | 4.982 | yes |
| Node | wasm | cold-library | random-100000-00006 | 2 / 4 | 243368 / 274609 | n/a / 4 | yes | 1221.043 | yes |
| Node | wasm | cold-library | random-100000-00007 | 2 / 4 | 277933 / 309174 | n/a / 4 | yes | 359.717 | yes |
| Node | wasm | cold-library | random-100000-00008 | 2 / 4 | 168785 / 200026 | n/a / 4 | yes | 739.452 | yes |
| Node | wasm | cold-library | random-100000-00009 | 2 / 4 | 208516 / 239757 | n/a / 4 | yes | 719.854 | yes |
| Node | wasm | cold-library | random-1000000-00000 | 2 / 4 | 1340288 / 1371529 | n/a / 4 | yes | 5333.008 | yes |
| Node | wasm | cold-library | random-1000000-00001 | 2 / 4 | 1623801 / 1655042 | n/a / 4 | yes | 7674.516 | yes |
| Node | wasm | cold-library | random-1000000-00002 | 2 / 4 | 1570680 / 1601921 | n/a / 4 | yes | 7046.683 | yes |
| Node | wasm | cold-library | random-1000000-00003 | 2 / 4 | 1131720 / 1162961 | n/a / 4 | yes | 3517.169 | yes |
| Node | wasm | cold-library | random-1000000-00004 | 2 / 4 | 1461198 / 1492439 | n/a / 4 | yes | 6167.738 | yes |
| Node | wasm | cold-library | random-1000000-00005 | 2 / 4 | 1656122 / 1687363 | n/a / 4 | yes | 8066.647 | yes |
| Node | wasm | cold-library | random-1000000-00006 | 2 / 4 | 1318887 / 1350128 | n/a / 4 | yes | 5407.190 | yes |
| Node | wasm | cold-library | random-1000000-00007 | 2 / 4 | 1657238 / 1688479 | n/a / 4 | yes | 6652.090 | yes |
| Node | wasm | cold-library | random-1000000-00008 | 2 / 4 | 1358111 / 1389352 | n/a / 4 | yes | 5439.411 | yes |
| Node | wasm | cold-library | random-1000000-00009 | 2 / 4 | 2321292 / 2352533 | n/a / 4 | yes | 13187.152 | yes |
| Node | wasm | warm-repeated-query | fixed-micb | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 334.729 | yes |
| Node | wasm | warm-repeated-query | fixed-kir3dl1 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 823.436 | yes |
| Node | wasm | warm-repeated-query | boundary-start | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 40.175 | yes |
| Node | wasm | warm-repeated-query | pan-anchor | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 219.077 | yes |
| Node | wasm | warm-repeated-query | nearby-pan | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 96.973 | yes |
| Node | wasm | warm-repeated-query | boundary-end | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 5.666 | yes |
| Node | wasm | warm-repeated-query | distant-random | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 0.735 | yes |
| Node | wasm | warm-repeated-query | random-1000-00000 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 3782.875 | yes |
| Node | wasm | warm-repeated-query | random-1000-00001 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 0.200 | yes |
| Node | wasm | warm-repeated-query | random-1000-00002 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 90.375 | yes |
| Node | wasm | warm-repeated-query | random-1000-00003 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 245.377 | yes |
| Node | wasm | warm-repeated-query | random-1000-00004 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 0.319 | yes |
| Node | wasm | warm-repeated-query | random-1000-00005 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 5274.064 | yes |
| Node | wasm | warm-repeated-query | random-1000-00006 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 0.319 | yes |
| Node | wasm | warm-repeated-query | random-1000-00007 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 0.172 | yes |
| Node | wasm | warm-repeated-query | random-1000-00008 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 0.195 | yes |
| Node | wasm | warm-repeated-query | random-1000-00009 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 74.352 | yes |
| Node | wasm | warm-repeated-query | random-10000-00000 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 2.242 | yes |
| Node | wasm | warm-repeated-query | random-10000-00001 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 267.138 | yes |
| Node | wasm | warm-repeated-query | random-10000-00002 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 13.091 | yes |
| Node | wasm | warm-repeated-query | random-10000-00003 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 121.487 | yes |
| Node | wasm | warm-repeated-query | random-10000-00004 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 160.468 | yes |
| Node | wasm | warm-repeated-query | random-10000-00005 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 317.812 | yes |
| Node | wasm | warm-repeated-query | random-10000-00006 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 143.444 | yes |
| Node | wasm | warm-repeated-query | random-10000-00007 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 120.123 | yes |
| Node | wasm | warm-repeated-query | random-10000-00008 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 202.755 | yes |
| Node | wasm | warm-repeated-query | random-10000-00009 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 86.322 | yes |
| Node | wasm | warm-repeated-query | random-100000-00000 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 4.011 | yes |
| Node | wasm | warm-repeated-query | random-100000-00001 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 2894.936 | yes |
| Node | wasm | warm-repeated-query | random-100000-00002 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 26.366 | yes |
| Node | wasm | warm-repeated-query | random-100000-00003 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 3.914 | yes |
| Node | wasm | warm-repeated-query | random-100000-00004 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 766.110 | yes |
| Node | wasm | warm-repeated-query | random-100000-00005 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 3.519 | yes |
| Node | wasm | warm-repeated-query | random-100000-00006 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 1229.935 | yes |
| Node | wasm | warm-repeated-query | random-100000-00007 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 352.341 | yes |
| Node | wasm | warm-repeated-query | random-100000-00008 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 700.394 | yes |
| Node | wasm | warm-repeated-query | random-100000-00009 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 708.156 | yes |
| Node | wasm | warm-repeated-query | random-1000000-00000 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 5214.221 | yes |
| Node | wasm | warm-repeated-query | random-1000000-00001 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 7534.691 | yes |
| Node | wasm | warm-repeated-query | random-1000000-00002 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 6879.346 | yes |
| Node | wasm | warm-repeated-query | random-1000000-00003 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 3539.328 | yes |
| Node | wasm | warm-repeated-query | random-1000000-00004 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 5951.502 | yes |
| Node | wasm | warm-repeated-query | random-1000000-00005 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 7806.486 | yes |
| Node | wasm | warm-repeated-query | random-1000000-00006 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 5290.240 | yes |
| Node | wasm | warm-repeated-query | random-1000000-00007 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 6814.986 | yes |
| Node | wasm | warm-repeated-query | random-1000000-00008 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 5699.860 | yes |
| Node | wasm | warm-repeated-query | random-1000000-00009 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 13468.933 | yes |

Latency p50/p95/max: 219.077 / 7674.516 / 13468.933 ms across 94 measurements.

## Decoder comparison

| Decoder | init ms | JS bytes | WASM bytes | chunk p50 ms | chunk p95 ms | peak heap bytes |
|---|---:|---:|---:|---:|---:|---:|
| wasm | 8.940 | 229706 | 251806 | 4.792 | 11.506 | 2309497528 |

The WASM decoder remains optional. Initialization, deployable asset bytes, per-chunk time, total query time, available memory evidence, and correctness all remain visible; steady-state decompression alone does not select the default.

## Limitations

- Positioned-file reads measure the reader and local storage path; they are not HTTP or browser measurements.
- Cold library mode creates a new archive reader per query; operating-system and remote CDN cache state are uncontrolled.
- Warm mode primes the exact query, retaining both directory and compressed-payload library caches within their configured byte budgets.
