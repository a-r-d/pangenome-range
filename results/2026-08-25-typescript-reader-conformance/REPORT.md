# TypeScript range reader and cross-language conformance

Date: 2026-08-25

Baseline commit: `6c77634`

## Outcome

The private `pangenome-range` package now opens archive versions 3 and 4 from
HTTP, Blob, memory, or a Node positioned file source. It dispatches all regional
payload versions retained by Rust (2, 3, and 4), returns typed-array-oriented
tiles, preserves anonymous haplotypes per tile, merges only valid canonical
graph/path state, and produces the same BLAKE3 hashes as the Rust reader.

No retained archive or regional version is unsupported. Unknown future versions
are rejected explicitly. The viewer was not changed.

## Fixture matrix

The Rust `fixtures export` command deterministically emits the complete
synthetic fixture matrix plus isolated archive sections and zstd levels 1, 3,
and 6. The Rust test exports twice, requires byte identity, and rereads every
archive. TypeScript decodes the same files and matches references, typed tile
data, semantic labels, and canonical hashes.

| Fixture | Archive | Payload | Semantics | Canonical hash |
|---|---:|---:|---|---|
| `archive-v3-named-v2` | 3 | 2 | named paths | `c56f5442...60528` |
| `archive-v4-weighted-v3` | 4 | 3 | weighted anonymous tile paths | `ef21ce8e...1fab1` |
| `archive-v4-record-v4` | 4 | 4 | reconstructed weighted anonymous tile paths | `ef21ce8e...1fab1` |

Negative and bounded generated tests cover truncated archive sections and
payloads, bad magic/version, u64 overflow, out-of-file payloads, invalid UTF-8,
bad dictionary indexes, incorrect decompressed lengths, unknown codecs,
unreasonable counts, malformed HTTP metadata, changed ETags, and incorrect
`200` whole-object responses.

## Real Node range integration

The integration archive is 164,266 bytes with SHA-256
`2c229cdea3d9a27686f08a4e1ceaf36790dff462ec8867191bc77cb0499526e6`.
Its sidecar records the pinned 73,920-byte source checksum and deterministic
encode command. A real loopback origin supplies `HEAD`, `206`, exposed CORS
headers, immutable/no-transform caching, and stable `ETag`; every data GET uses
`If-Range`.

| Query | Tiles | Nodes | Traversals | GETs | Bytes | Canonical hash |
|---|---:|---:|---:|---:|---:|---|
| MICB | 2 | 659 | 94 | 2 | 37,797 | `afb9deec...8c3d2` |
| KIR3DL1 | 2 | 2,231 | 67 | 3 | 68,619 | `62868752...c2411` |

The exact HTTP range list equals the reader query trace. `BlobRangeSource` and
`FileRangeSource` return the same canonical hash and selected node count for
both queries.

## Browser transport gate

Chromium, Firefox, and WebKit each opened the same deterministic 70,024-byte
synthetic archive (`3dc585b8...0c4ef`), issued one successful `HEAD`, and then
fetched exactly three `206` ranges totaling 20,604 bytes:

```text
bytes=0-16383
bytes=65804-69899
bytes=69900-70023
```

All engines decoded one two-node, one-traversal tile. These are loopback
functional measurements, not browser or CDN latency claims.

## Package output

| Entry | Raw bytes | Gzip bytes | Budget |
|---|---:|---:|---|
| reader/root | 138,924 | 31,195 | 160 KiB raw / 50 KiB gzip |
| Node | 1,634 | 638 | 8 KiB raw |
| viewer placeholder | 273 | 192 | informational |

Root and `/reader` remain browser-only and contain no Node built-ins. `/node`
contains `FileRangeSource`; the export smoke proves that it is absent from the
root. ESM, declaration, sourcemap, and `sideEffects: false` checks pass.

## Commands

```bash
cargo run --release -p pangenome-range-cli -- fixtures export test-data/conformance
pnpm --filter @pangenome-range/benchmark test
pnpm test:browser
pnpm --filter pangenome-range test:bundle
pnpm lint
pnpm typecheck
pnpm test
pnpm build
cargo test --workspace
```

The complete command results are summarized in `summary.json`. Timing is not
retained as performance evidence because all HTTP tests use a local origin.

## Remaining limitations

- The package remains private and the viewer remains unimplemented.
- Query context cannot exceed the archive's fixed 100-base construction halo.
- Payload cache entries are compressed bytes; repeated queries still
  decompress and decode cached chunks.
- There is no public-network cold/warm browser corpus yet.
- Archive sections have structural bounds checks but no per-section checksum or
  authentication beyond externally tracked whole-object identity.
