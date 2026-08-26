# Range benchmark comparison

The real-runtime table uses origin/source observations for actual request counts, bytes, and request rounds; reader traces provide planned dependency rounds, and runtime phase clocks provide decode/total latency. It does not combine those values with Rust's optimistic network model.

| run | runtime | engines | decoders | planned rounds p50 | observed rounds p50 | actual ranges p50 | actual bytes p50 | decode p50 ms | end-to-end p50 ms |
|---|---|---|---|---:|---:|---:|---:|---:|---:|
| 2026-08-25-node-benchmark-harness-smoke | node | Node | pure-js, wasm | 1.000 | n/a | 0.000 | 0.000 | 15.636 | 20.622 |
| 2026-08-25-browser-benchmark-harness-smoke | browser | chromium, firefox, webkit | pure-js, wasm | 2.000 | 1.000 | 1.000 | 21451.000 | 22.000 | 46.000 |

## Rust simulated layout evidence

Source: `/media/ard/samsung-2tb/projects/pangenome-range/results/2026-08-25-browser-benchmark-harness-smoke/rust-verification.json`. These are Rust local decode measurements plus the explicit 20 ms simulated network model. They are not Node or browser observations.

| rows | planned rounds p50 | planned ranges p50 | planned bytes p50 | local Rust decode p50 ms | simulated 20 ms profile p50 ms |
|---:|---:|---:|---:|---:|---:|
| 9 | 1.000 | 1.000 | 21451.000 | 5.463 | 21.072 |

## Cold/warm and per-browser detail

### 2026-08-25-node-benchmark-harness-smoke

- Node / pure-js / cold: 10 queries; latency p50/p95 24.865/106.665 ms; actual bytes p50 21413.000.
- Node / pure-js / warm: 10 queries; latency p50/p95 22.415/94.180 ms; actual bytes p50 0.000.
- Node / wasm / cold: 10 queries; latency p50/p95 20.467/87.812 ms; actual bytes p50 21413.000.
- Node / wasm / warm: 10 queries; latency p50/p95 20.461/81.913 ms; actual bytes p50 0.000.

### 2026-08-25-browser-benchmark-harness-smoke

- chromium / pure-js / cold: 2 queries; latency p50/p95 66.400/70.300 ms; actual bytes p50 37797.000.
- chromium / pure-js / warm: 4 queries; latency p50/p95 29.700/80.700 ms; actual bytes p50 21413.000.
- chromium / wasm / cold: 2 queries; latency p50/p95 76.900/79.500 ms; actual bytes p50 37797.000.
- chromium / wasm / warm: 4 queries; latency p50/p95 29.600/70.700 ms; actual bytes p50 21413.000.
- firefox / pure-js / cold: 2 queries; latency p50/p95 74.000/179.000 ms; actual bytes p50 37797.000.
- firefox / pure-js / warm: 4 queries; latency p50/p95 34.000/119.000 ms; actual bytes p50 21413.000.
- firefox / wasm / cold: 2 queries; latency p50/p95 64.000/74.000 ms; actual bytes p50 37797.000.
- firefox / wasm / warm: 4 queries; latency p50/p95 31.000/84.000 ms; actual bytes p50 21413.000.
- webkit / pure-js / cold: 2 queries; latency p50/p95 63.000/64.000 ms; actual bytes p50 37797.000.
- webkit / pure-js / warm: 4 queries; latency p50/p95 30.000/101.000 ms; actual bytes p50 21413.000.
- webkit / wasm / cold: 2 queries; latency p50/p95 68.000/69.000 ms; actual bytes p50 37797.000.
- webkit / wasm / warm: 4 queries; latency p50/p95 25.000/96.000 ms; actual bytes p50 21413.000.
