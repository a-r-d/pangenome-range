# Results

This directory will hold retained benchmark evidence, not hand-edited headline
numbers. Each result set should include a README or manifest with:

- source data URL, size, and checksum;
- pangenome-range and upstream commits plus `Cargo.lock` hash;
- release-build command and host/storage/network details;
- query corpus and random seed;
- raw per-query range traces, timings, canonical hashes, and failures;
- aggregate-generation command.

TypeScript/Playwright benchmark runs use this fixed compact layout:

```text
results/<run-id>/
  config.json
  environment.json
  requests.ndjson
  queries.csv
  summary.json
  REPORT.md
```

Runs may add machine-generated provenance or derived reports such as
`archive-build.json`, `rust-verification.json`, and `COMPARISON.md`; these do
not replace the six standard raw/runtime artifacts.

The private benchmark CLI creates the run directory exclusively and will not
overwrite retained evidence. Rust simulated network estimates and real
Node/browser measurements must remain separate columns/sections in comparison
reports.

Do not commit large outputs or third-party data without an explicit size and
licensing decision.
