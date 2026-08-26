# Range benchmark comparison

The real-runtime table uses origin/source observations for actual request counts, bytes, and request rounds; reader traces provide planned dependency rounds, and runtime phase clocks provide decode/total latency. It does not combine those values with Rust's optimistic network model.

| run | runtime | engines | decoders | planned rounds p50 | observed rounds p50 | actual ranges p50 | actual bytes p50 | decode p50 ms | end-to-end p50 ms |
|---|---|---|---|---:|---:|---:|---:|---:|---:|
| 2026-08-25-node-mhc-benchmark-smoke | node | Node | pure-js, wasm | 1.000 | n/a | 0.000 | 0.000 | 2.066 | 6.367 |
| 2026-08-25-browser-mhc-benchmark-smoke | browser | chromium, firefox, webkit | pure-js, wasm | 2.000 | 1.000 | 1.000 | 7715.000 | 2.200 | 11.000 |

## Rust simulated layout evidence

Source: `/media/ard/samsung-2tb/projects/pangenome-range/results/2026-08-25-browser-mhc-benchmark-smoke/rust-verification.json`. These are Rust local decode measurements plus the explicit 20 ms simulated network model. They are not Node or browser observations.

| rows | planned rounds p50 | planned ranges p50 | planned bytes p50 | local Rust decode p50 ms | simulated 20 ms profile p50 ms |
|---:|---:|---:|---:|---:|---:|
| 13 | 2.000 | 2.000 | 18556.000 | 0.592 | 41.391 |

## Cold/warm and per-browser detail

### 2026-08-25-node-mhc-benchmark-smoke

- Node / pure-js / cold: 14 queries; latency p50/p95 11.668/1875.187 ms; actual bytes p50 18549.000.
- Node / pure-js / warm: 14 queries; latency p50/p95 5.978/1719.308 ms; actual bytes p50 0.000.
- Node / wasm / cold: 14 queries; latency p50/p95 4.726/1482.071 ms; actual bytes p50 18549.000.
- Node / wasm / warm: 14 queries; latency p50/p95 4.256/1580.007 ms; actual bytes p50 0.000.

### 2026-08-25-browser-mhc-benchmark-smoke

- chromium / pure-js / cold: 2 queries; latency p50/p95 20.000/24.500 ms; actual bytes p50 24099.000.
- chromium / pure-js / warm: 4 queries; latency p50/p95 7.200/25.500 ms; actual bytes p50 0.000.
- chromium / wasm / cold: 2 queries; latency p50/p95 26.600/27.000 ms; actual bytes p50 24099.000.
- chromium / wasm / warm: 4 queries; latency p50/p95 5.100/22.400 ms; actual bytes p50 0.000.
- firefox / pure-js / cold: 2 queries; latency p50/p95 26.000/99.000 ms; actual bytes p50 24099.000.
- firefox / pure-js / warm: 4 queries; latency p50/p95 8.000/33.000 ms; actual bytes p50 0.000.
- firefox / wasm / cold: 2 queries; latency p50/p95 24.000/24.000 ms; actual bytes p50 24099.000.
- firefox / wasm / warm: 4 queries; latency p50/p95 6.000/23.000 ms; actual bytes p50 0.000.
- webkit / pure-js / cold: 2 queries; latency p50/p95 20.000/21.000 ms; actual bytes p50 24099.000.
- webkit / pure-js / warm: 4 queries; latency p50/p95 6.000/27.000 ms; actual bytes p50 0.000.
- webkit / wasm / cold: 2 queries; latency p50/p95 24.000/24.000 ms; actual bytes p50 24099.000.
- webkit / wasm / warm: 4 queries; latency p50/p95 4.000/23.000 ms; actual bytes p50 0.000.
