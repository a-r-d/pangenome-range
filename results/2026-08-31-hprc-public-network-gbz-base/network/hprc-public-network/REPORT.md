# Node range benchmark: hprc-public-network

Status: **correctness gate passed**.

This report contains real HTTP measurements. Planned reader ranges and dependency rounds are reported separately from requests actually observed at the transport/origin. No simulated Rust latency is relabeled as Node or browser performance.

## Archive and workload

- Location: `https://archives.ard.ninja/pangenome-range/sha256/82585cb612effbf414b1c8f38b049bc415876866168ccc929f9a885f06d97b0a/hprc-v2.1-gencode-v50-named-membership-82585cb612effbf4.pngr`
- Size: 10836425558 bytes
- Archive SHA-256: 82585cb612effbf414b1c8f38b049bc415876866168ccc929f9a885f06d97b0a
- ETag: "6a95a096-285e6bb56"
- Workload SHA-256: `c2a88ba48b0221ed69fc8d7c09c221dedacacf4f2d3a40d44e0faf682aacfaba`

## Query measurements

| Runtime | Decoder | Scenario | Query | actual/planned requests | actual/planned bytes | actual/planned rounds | source/origin match | total ms | correct |
|---|---|---|---|---:|---:|---:|---:|---:|---|
| Node | pure-js | cold-library | fixed-micb | 2 / 4 | 71553 / 102794 | n/a / 4 | yes | 741.879 | yes |
| Node | pure-js | cold-library | fixed-kir3dl1 | 2 / 4 | 127912 / 159153 | n/a / 4 | yes | 1504.226 | yes |
| Node | pure-js | cold-library | boundary-start | 2 / 4 | 15314 / 46555 | n/a / 4 | yes | 331.515 | yes |
| Node | pure-js | cold-library | pan-anchor | 2 / 4 | 56552 / 87793 | n/a / 4 | yes | 569.790 | yes |
| Node | pure-js | cold-library | nearby-pan | 2 / 4 | 28389 / 59630 | n/a / 4 | yes | 421.071 | yes |
| Node | pure-js | cold-library | boundary-end | 2 / 4 | 17802 / 49043 | n/a / 4 | yes | 304.977 | yes |
| Node | pure-js | cold-library | distant-random | 2 / 4 | 4382 / 35623 | n/a / 4 | yes | 298.896 | yes |
| Node | pure-js | cold-library | random-1000-00000 | 2 / 4 | 180102 / 211343 | n/a / 4 | yes | 4765.225 | yes |
| Node | pure-js | cold-library | random-1000-00001 | 2 / 4 | 4675 / 35916 | n/a / 4 | yes | 281.724 | yes |
| Node | pure-js | cold-library | random-1000-00002 | 2 / 4 | 25847 / 57088 | n/a / 4 | yes | 408.100 | yes |
| Node | pure-js | cold-library | random-1000-00003 | 2 / 4 | 40880 / 72121 | n/a / 4 | yes | 613.899 | yes |
| Node | pure-js | cold-library | random-1000-00004 | 2 / 4 | 9818 / 41059 | n/a / 4 | yes | 300.701 | yes |
| Node | pure-js | cold-library | random-1000-00005 | 2 / 4 | 402128 / 433369 | n/a / 4 | yes | 8063.725 | yes |
| Node | pure-js | cold-library | random-1000-00006 | 2 / 4 | 7800 / 39041 | n/a / 4 | yes | 434.667 | yes |
| Node | pure-js | cold-library | random-1000-00007 | 2 / 4 | 4649 / 35890 | n/a / 4 | yes | 283.834 | yes |
| Node | pure-js | cold-library | random-1000-00008 | 2 / 4 | 4691 / 35932 | n/a / 4 | yes | 283.003 | yes |
| Node | pure-js | cold-library | random-1000-00009 | 2 / 4 | 27474 / 58715 | n/a / 4 | yes | 457.995 | yes |
| Node | pure-js | cold-library | random-10000-00000 | 2 / 4 | 19313 / 50554 | n/a / 4 | yes | 311.838 | yes |
| Node | pure-js | cold-library | random-10000-00001 | 2 / 4 | 58878 / 90119 | n/a / 4 | yes | 605.206 | yes |
| Node | pure-js | cold-library | random-10000-00002 | 2 / 4 | 31420 / 62661 | n/a / 4 | yes | 308.102 | yes |
| Node | pure-js | cold-library | random-10000-00003 | 2 / 4 | 46117 / 77358 | n/a / 4 | yes | 438.120 | yes |
| Node | pure-js | cold-library | random-10000-00004 | 2 / 4 | 46386 / 77627 | n/a / 4 | yes | 458.363 | yes |
| Node | pure-js | cold-library | random-10000-00005 | 2 / 4 | 66191 / 97432 | n/a / 4 | yes | 644.776 | yes |
| Node | pure-js | cold-library | random-10000-00006 | 2 / 4 | 32642 / 63883 | n/a / 4 | yes | 456.359 | yes |
| Node | pure-js | cold-library | random-10000-00007 | 2 / 4 | 30543 / 61784 | n/a / 4 | yes | 423.712 | yes |
| Node | pure-js | cold-library | random-10000-00008 | 2 / 4 | 56901 / 88142 | n/a / 4 | yes | 546.405 | yes |
| Node | pure-js | cold-library | random-10000-00009 | 2 / 4 | 34250 / 65491 | n/a / 4 | yes | 397.506 | yes |
| Node | pure-js | cold-library | random-100000-00000 | 2 / 4 | 41729 / 72970 | n/a / 4 | yes | 295.303 | yes |
| Node | pure-js | cold-library | random-100000-00001 | 2 / 4 | 481072 / 512313 | n/a / 4 | yes | 3360.550 | yes |
| Node | pure-js | cold-library | random-100000-00002 | 2 / 4 | 66921 / 98162 | n/a / 4 | yes | 359.976 | yes |
| Node | pure-js | cold-library | random-100000-00003 | 2 / 4 | 36506 / 67747 | n/a / 4 | yes | 307.442 | yes |
| Node | pure-js | cold-library | random-100000-00004 | 2 / 4 | 196383 / 227624 | n/a / 4 | yes | 1156.153 | yes |
| Node | pure-js | cold-library | random-100000-00005 | 2 / 4 | 13331 / 44572 | n/a / 4 | yes | 298.923 | yes |
| Node | pure-js | cold-library | random-100000-00006 | 2 / 4 | 243368 / 274609 | n/a / 4 | yes | 1602.284 | yes |
| Node | pure-js | cold-library | random-100000-00007 | 2 / 4 | 277933 / 309174 | n/a / 4 | yes | 722.661 | yes |
| Node | pure-js | cold-library | random-100000-00008 | 2 / 4 | 168785 / 200026 | n/a / 4 | yes | 1026.346 | yes |
| Node | pure-js | cold-library | random-100000-00009 | 2 / 4 | 208516 / 239757 | n/a / 4 | yes | 1098.040 | yes |
| Node | pure-js | cold-library | random-1000000-00000 | 2 / 4 | 1340288 / 1371529 | n/a / 4 | yes | 6010.428 | yes |
| Node | pure-js | cold-library | random-1000000-00001 | 2 / 4 | 1623801 / 1655042 | n/a / 4 | yes | 8378.785 | yes |
| Node | pure-js | cold-library | random-1000000-00002 | 2 / 4 | 1570680 / 1601921 | n/a / 4 | yes | 7564.117 | yes |
| Node | pure-js | cold-library | random-1000000-00003 | 2 / 4 | 1131720 / 1162961 | n/a / 4 | yes | 3935.234 | yes |
| Node | pure-js | cold-library | random-1000000-00004 | 2 / 4 | 1461198 / 1492439 | n/a / 4 | yes | 6863.722 | yes |
| Node | pure-js | cold-library | random-1000000-00005 | 2 / 4 | 1656122 / 1687363 | n/a / 4 | yes | 8843.637 | yes |
| Node | pure-js | cold-library | random-1000000-00006 | 2 / 4 | 1318887 / 1350128 | n/a / 4 | yes | 6155.394 | yes |
| Node | pure-js | cold-library | random-1000000-00007 | 2 / 4 | 1657238 / 1688479 | n/a / 4 | yes | 7796.217 | yes |
| Node | pure-js | cold-library | random-1000000-00008 | 2 / 4 | 1358111 / 1389352 | n/a / 4 | yes | 6398.172 | yes |
| Node | pure-js | cold-library | random-1000000-00009 | 2 / 4 | 2321292 / 2352533 | n/a / 4 | yes | 15013.887 | yes |
| Node | pure-js | warm-repeated-query | fixed-micb | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 335.308 | yes |
| Node | pure-js | warm-repeated-query | fixed-kir3dl1 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 865.545 | yes |
| Node | pure-js | warm-repeated-query | boundary-start | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 47.525 | yes |
| Node | pure-js | warm-repeated-query | pan-anchor | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 222.068 | yes |
| Node | pure-js | warm-repeated-query | nearby-pan | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 119.502 | yes |
| Node | pure-js | warm-repeated-query | boundary-end | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 10.176 | yes |
| Node | pure-js | warm-repeated-query | distant-random | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 1.091 | yes |
| Node | pure-js | warm-repeated-query | random-1000-00000 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 4147.646 | yes |
| Node | pure-js | warm-repeated-query | random-1000-00001 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 0.290 | yes |
| Node | pure-js | warm-repeated-query | random-1000-00002 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 102.451 | yes |
| Node | pure-js | warm-repeated-query | random-1000-00003 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 258.480 | yes |
| Node | pure-js | warm-repeated-query | random-1000-00004 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 1.011 | yes |
| Node | pure-js | warm-repeated-query | random-1000-00005 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 5918.293 | yes |
| Node | pure-js | warm-repeated-query | random-1000-00006 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 0.927 | yes |
| Node | pure-js | warm-repeated-query | random-1000-00007 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 0.290 | yes |
| Node | pure-js | warm-repeated-query | random-1000-00008 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 0.329 | yes |
| Node | pure-js | warm-repeated-query | random-1000-00009 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 79.849 | yes |
| Node | pure-js | warm-repeated-query | random-10000-00000 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 4.196 | yes |
| Node | pure-js | warm-repeated-query | random-10000-00001 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 283.334 | yes |
| Node | pure-js | warm-repeated-query | random-10000-00002 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 17.005 | yes |
| Node | pure-js | warm-repeated-query | random-10000-00003 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 132.233 | yes |
| Node | pure-js | warm-repeated-query | random-10000-00004 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 172.135 | yes |
| Node | pure-js | warm-repeated-query | random-10000-00005 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 305.831 | yes |
| Node | pure-js | warm-repeated-query | random-10000-00006 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 140.654 | yes |
| Node | pure-js | warm-repeated-query | random-10000-00007 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 120.367 | yes |
| Node | pure-js | warm-repeated-query | random-10000-00008 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 213.723 | yes |
| Node | pure-js | warm-repeated-query | random-10000-00009 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 75.181 | yes |
| Node | pure-js | warm-repeated-query | random-100000-00000 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 9.727 | yes |
| Node | pure-js | warm-repeated-query | random-100000-00001 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 3002.397 | yes |
| Node | pure-js | warm-repeated-query | random-100000-00002 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 30.202 | yes |
| Node | pure-js | warm-repeated-query | random-100000-00003 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 8.608 | yes |
| Node | pure-js | warm-repeated-query | random-100000-00004 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 799.428 | yes |
| Node | pure-js | warm-repeated-query | random-100000-00005 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 7.494 | yes |
| Node | pure-js | warm-repeated-query | random-100000-00006 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 1340.678 | yes |
| Node | pure-js | warm-repeated-query | random-100000-00007 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 406.340 | yes |
| Node | pure-js | warm-repeated-query | random-100000-00008 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 701.753 | yes |
| Node | pure-js | warm-repeated-query | random-100000-00009 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 793.381 | yes |
| Node | pure-js | warm-repeated-query | random-1000000-00000 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 5633.057 | yes |
| Node | pure-js | warm-repeated-query | random-1000000-00001 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 7868.238 | yes |
| Node | pure-js | warm-repeated-query | random-1000000-00002 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 7270.856 | yes |
| Node | pure-js | warm-repeated-query | random-1000000-00003 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 3568.035 | yes |
| Node | pure-js | warm-repeated-query | random-1000000-00004 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 6393.117 | yes |
| Node | pure-js | warm-repeated-query | random-1000000-00005 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 8361.852 | yes |
| Node | pure-js | warm-repeated-query | random-1000000-00006 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 5551.640 | yes |
| Node | pure-js | warm-repeated-query | random-1000000-00007 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 6973.232 | yes |
| Node | pure-js | warm-repeated-query | random-1000000-00008 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 5796.431 | yes |
| Node | pure-js | warm-repeated-query | random-1000000-00009 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 13584.744 | yes |
| Node | wasm | cold-library | fixed-micb | 2 / 4 | 71553 / 102794 | n/a / 4 | yes | 681.168 | yes |
| Node | wasm | cold-library | fixed-kir3dl1 | 2 / 4 | 127912 / 159153 | n/a / 4 | yes | 1178.504 | yes |
| Node | wasm | cold-library | boundary-start | 2 / 4 | 15314 / 46555 | n/a / 4 | yes | 354.711 | yes |
| Node | wasm | cold-library | pan-anchor | 2 / 4 | 56552 / 87793 | n/a / 4 | yes | 551.481 | yes |
| Node | wasm | cold-library | nearby-pan | 2 / 4 | 28389 / 59630 | n/a / 4 | yes | 399.094 | yes |
| Node | wasm | cold-library | boundary-end | 2 / 4 | 17802 / 49043 | n/a / 4 | yes | 285.009 | yes |
| Node | wasm | cold-library | distant-random | 2 / 4 | 4382 / 35623 | n/a / 4 | yes | 301.877 | yes |
| Node | wasm | cold-library | random-1000-00000 | 2 / 4 | 180102 / 211343 | n/a / 4 | yes | 4472.710 | yes |
| Node | wasm | cold-library | random-1000-00001 | 2 / 4 | 4675 / 35916 | n/a / 4 | yes | 327.281 | yes |
| Node | wasm | cold-library | random-1000-00002 | 2 / 4 | 25847 / 57088 | n/a / 4 | yes | 384.261 | yes |
| Node | wasm | cold-library | random-1000-00003 | 2 / 4 | 40880 / 72121 | n/a / 4 | yes | 837.927 | yes |
| Node | wasm | cold-library | random-1000-00004 | 2 / 4 | 9818 / 41059 | n/a / 4 | yes | 286.142 | yes |
| Node | wasm | cold-library | random-1000-00005 | 2 / 4 | 402128 / 433369 | n/a / 4 | yes | 6063.000 | yes |
| Node | wasm | cold-library | random-1000-00006 | 2 / 4 | 7800 / 39041 | n/a / 4 | yes | 298.260 | yes |
| Node | wasm | cold-library | random-1000-00007 | 2 / 4 | 4649 / 35890 | n/a / 4 | yes | 325.738 | yes |
| Node | wasm | cold-library | random-1000-00008 | 2 / 4 | 4691 / 35932 | n/a / 4 | yes | 317.877 | yes |
| Node | wasm | cold-library | random-1000-00009 | 2 / 4 | 27474 / 58715 | n/a / 4 | yes | 388.551 | yes |
| Node | wasm | cold-library | random-10000-00000 | 2 / 4 | 19313 / 50554 | n/a / 4 | yes | 290.248 | yes |
| Node | wasm | cold-library | random-10000-00001 | 2 / 4 | 58878 / 90119 | n/a / 4 | yes | 544.588 | yes |
| Node | wasm | cold-library | random-10000-00002 | 2 / 4 | 31420 / 62661 | n/a / 4 | yes | 303.276 | yes |
| Node | wasm | cold-library | random-10000-00003 | 2 / 4 | 46117 / 77358 | n/a / 4 | yes | 415.950 | yes |
| Node | wasm | cold-library | random-10000-00004 | 2 / 4 | 46386 / 77627 | n/a / 4 | yes | 447.395 | yes |
| Node | wasm | cold-library | random-10000-00005 | 2 / 4 | 66191 / 97432 | n/a / 4 | yes | 610.352 | yes |
| Node | wasm | cold-library | random-10000-00006 | 2 / 4 | 32642 / 63883 | n/a / 4 | yes | 421.035 | yes |
| Node | wasm | cold-library | random-10000-00007 | 2 / 4 | 30543 / 61784 | n/a / 4 | yes | 428.223 | yes |
| Node | wasm | cold-library | random-10000-00008 | 2 / 4 | 56901 / 88142 | n/a / 4 | yes | 496.951 | yes |
| Node | wasm | cold-library | random-10000-00009 | 2 / 4 | 34250 / 65491 | n/a / 4 | yes | 371.154 | yes |
| Node | wasm | cold-library | random-100000-00000 | 2 / 4 | 41729 / 72970 | n/a / 4 | yes | 305.527 | yes |
| Node | wasm | cold-library | random-100000-00001 | 2 / 4 | 481072 / 512313 | n/a / 4 | yes | 3319.190 | yes |
| Node | wasm | cold-library | random-100000-00002 | 2 / 4 | 66921 / 98162 | n/a / 4 | yes | 341.455 | yes |
| Node | wasm | cold-library | random-100000-00003 | 2 / 4 | 36506 / 67747 | n/a / 4 | yes | 291.600 | yes |
| Node | wasm | cold-library | random-100000-00004 | 2 / 4 | 196383 / 227624 | n/a / 4 | yes | 1088.034 | yes |
| Node | wasm | cold-library | random-100000-00005 | 2 / 4 | 13331 / 44572 | n/a / 4 | yes | 285.787 | yes |
| Node | wasm | cold-library | random-100000-00006 | 2 / 4 | 243368 / 274609 | n/a / 4 | yes | 1571.751 | yes |
| Node | wasm | cold-library | random-100000-00007 | 2 / 4 | 277933 / 309174 | n/a / 4 | yes | 706.705 | yes |
| Node | wasm | cold-library | random-100000-00008 | 2 / 4 | 168785 / 200026 | n/a / 4 | yes | 1024.994 | yes |
| Node | wasm | cold-library | random-100000-00009 | 2 / 4 | 208516 / 239757 | n/a / 4 | yes | 1119.326 | yes |
| Node | wasm | cold-library | random-1000000-00000 | 2 / 4 | 1340288 / 1371529 | n/a / 4 | yes | 5748.502 | yes |
| Node | wasm | cold-library | random-1000000-00001 | 2 / 4 | 1623801 / 1655042 | n/a / 4 | yes | 8460.871 | yes |
| Node | wasm | cold-library | random-1000000-00002 | 2 / 4 | 1570680 / 1601921 | n/a / 4 | yes | 7559.324 | yes |
| Node | wasm | cold-library | random-1000000-00003 | 2 / 4 | 1131720 / 1162961 | n/a / 4 | yes | 3812.593 | yes |
| Node | wasm | cold-library | random-1000000-00004 | 2 / 4 | 1461198 / 1492439 | n/a / 4 | yes | 6649.885 | yes |
| Node | wasm | cold-library | random-1000000-00005 | 2 / 4 | 1656122 / 1687363 | n/a / 4 | yes | 8669.586 | yes |
| Node | wasm | cold-library | random-1000000-00006 | 2 / 4 | 1318887 / 1350128 | n/a / 4 | yes | 5981.355 | yes |
| Node | wasm | cold-library | random-1000000-00007 | 2 / 4 | 1657238 / 1688479 | n/a / 4 | yes | 7397.715 | yes |
| Node | wasm | cold-library | random-1000000-00008 | 2 / 4 | 1358111 / 1389352 | n/a / 4 | yes | 6113.261 | yes |
| Node | wasm | cold-library | random-1000000-00009 | 2 / 4 | 2321292 / 2352533 | n/a / 4 | yes | 14269.186 | yes |
| Node | wasm | warm-repeated-query | fixed-micb | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 348.019 | yes |
| Node | wasm | warm-repeated-query | fixed-kir3dl1 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 888.147 | yes |
| Node | wasm | warm-repeated-query | boundary-start | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 41.012 | yes |
| Node | wasm | warm-repeated-query | pan-anchor | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 227.296 | yes |
| Node | wasm | warm-repeated-query | nearby-pan | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 94.079 | yes |
| Node | wasm | warm-repeated-query | boundary-end | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 5.859 | yes |
| Node | wasm | warm-repeated-query | distant-random | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 0.566 | yes |
| Node | wasm | warm-repeated-query | random-1000-00000 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 3724.633 | yes |
| Node | wasm | warm-repeated-query | random-1000-00001 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 0.245 | yes |
| Node | wasm | warm-repeated-query | random-1000-00002 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 85.104 | yes |
| Node | wasm | warm-repeated-query | random-1000-00003 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 262.391 | yes |
| Node | wasm | warm-repeated-query | random-1000-00004 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 0.362 | yes |
| Node | wasm | warm-repeated-query | random-1000-00005 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 5995.752 | yes |
| Node | wasm | warm-repeated-query | random-1000-00006 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 0.366 | yes |
| Node | wasm | warm-repeated-query | random-1000-00007 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 0.243 | yes |
| Node | wasm | warm-repeated-query | random-1000-00008 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 0.247 | yes |
| Node | wasm | warm-repeated-query | random-1000-00009 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 85.426 | yes |
| Node | wasm | warm-repeated-query | random-10000-00000 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 2.281 | yes |
| Node | wasm | warm-repeated-query | random-10000-00001 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 279.582 | yes |
| Node | wasm | warm-repeated-query | random-10000-00002 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 13.342 | yes |
| Node | wasm | warm-repeated-query | random-10000-00003 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 123.670 | yes |
| Node | wasm | warm-repeated-query | random-10000-00004 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 158.121 | yes |
| Node | wasm | warm-repeated-query | random-10000-00005 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 293.005 | yes |
| Node | wasm | warm-repeated-query | random-10000-00006 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 126.830 | yes |
| Node | wasm | warm-repeated-query | random-10000-00007 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 111.022 | yes |
| Node | wasm | warm-repeated-query | random-10000-00008 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 187.081 | yes |
| Node | wasm | warm-repeated-query | random-10000-00009 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 85.102 | yes |
| Node | wasm | warm-repeated-query | random-100000-00000 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 3.813 | yes |
| Node | wasm | warm-repeated-query | random-100000-00001 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 2836.617 | yes |
| Node | wasm | warm-repeated-query | random-100000-00002 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 20.805 | yes |
| Node | wasm | warm-repeated-query | random-100000-00003 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 3.898 | yes |
| Node | wasm | warm-repeated-query | random-100000-00004 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 745.430 | yes |
| Node | wasm | warm-repeated-query | random-100000-00005 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 11.478 | yes |
| Node | wasm | warm-repeated-query | random-100000-00006 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 1209.988 | yes |
| Node | wasm | warm-repeated-query | random-100000-00007 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 377.770 | yes |
| Node | wasm | warm-repeated-query | random-100000-00008 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 674.288 | yes |
| Node | wasm | warm-repeated-query | random-100000-00009 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 765.703 | yes |
| Node | wasm | warm-repeated-query | random-1000000-00000 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 5309.467 | yes |
| Node | wasm | warm-repeated-query | random-1000000-00001 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 7911.590 | yes |
| Node | wasm | warm-repeated-query | random-1000000-00002 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 7427.690 | yes |
| Node | wasm | warm-repeated-query | random-1000000-00003 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 3464.714 | yes |
| Node | wasm | warm-repeated-query | random-1000000-00004 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 5925.973 | yes |
| Node | wasm | warm-repeated-query | random-1000000-00005 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 8363.304 | yes |
| Node | wasm | warm-repeated-query | random-1000000-00006 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 5174.613 | yes |
| Node | wasm | warm-repeated-query | random-1000000-00007 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 6903.859 | yes |
| Node | wasm | warm-repeated-query | random-1000000-00008 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 5226.923 | yes |
| Node | wasm | warm-repeated-query | random-1000000-00009 | 0 / 2 | 0 / 31241 | n/a / 2 | yes | 13455.744 | yes |

Latency p50/p95/max: 421.071 / 8361.852 / 15013.887 ms across 188 measurements.

## Decoder comparison

| Decoder | init ms | JS bytes | WASM bytes | chunk p50 ms | chunk p95 ms | peak heap bytes |
|---|---:|---:|---:|---:|---:|---:|
| pure-js | 0.030 | 205299 | 0 | 85.125 | 214.527 | 2220142680 |
| wasm | 6.300 | 229706 | 251806 | 4.559 | 11.237 | 1817069360 |

The WASM decoder remains optional. Initialization, deployable asset bytes, per-chunk time, total query time, available memory evidence, and correctness all remain visible; steady-state decompression alone does not select the default.

## Limitations

- Remote archive identity uses the workload checksum plus live size/ETag; the benchmark does not download the complete object to recompute SHA-256.
- Cold library mode creates a new archive reader per query; operating-system and remote CDN cache state are uncontrolled.
- Warm mode primes the exact query, retaining both directory and compressed-payload library caches within their configured byte budgets.
