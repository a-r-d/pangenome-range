# Record-preserving v4 whole-source encoder report

Status: **accepted construction architecture; full archive completed**.

The normal encoder now stores exact compressed tile-local GBWT records plus
canonical graph topology and reconstructs weighted anonymous paths only in the
reader. This removes the interval-size collapse caused by enumerating and
sorting every local traversal for every tile. The full 5.49 GB HPRC v2.1 source
completed as an 8.23 GiB `.pngr` with zero occurrence-index, spool, or scratch
bytes.

Environment: rustc 1.97.1, Linux 6.17.0-41-generic x86_64, Intel Core i7-10700
(8 cores / 16 threads). `config.json` records the source URL, checksum, version,
data-use caveat, format versions, and exact normalized command. Raw report and
`/usr/bin/time -v` output remain beside the external 8.23 GiB archive; compact
metrics are retained in `summary.json`.

## Correctness and format gate

- `PNGRGN04` is a new regional version; `PNGRGN02` and `PNGRGN03` remain
  explicit Rust compatibility decoders and are never reinterpreted.
- Rust record decoding matched the independent GBZ-base JSON oracle exactly for
  nodes, boundary topology, reference coordinates/traversal, and weighted local
  traversals at four separated MHC tiles.
- One- and four-thread MHC archives were byte-identical.
- Rust and TypeScript decode the same raw golden payload. The suite retains its
  zstd-3 bytes, expected decoded JSON, BLAKE3, malformed fixtures, and a tiny
  deterministic `.pngr` rebuilt from the pinned MICB/KIR3DL1 source.
- The full archive passed decompression and structural validation for all
  363,105 physical payloads before atomic rename.
- A post-build query for `CHM13#chr1:1,000,000-1,100,000` independently
  extracted the source oracle, checked all seven selected tile-local haplotype
  results, and matched the archive canonical result. Its hash was
  `cbf983e845fcd6adcb1504089aba3c80fae85cd0c3998bcc90ba02f8fac8c5b4`.
  Structural validation and this semantic verification remain separate gates.

## Whole-source result

| Measurement | Result |
|---|---:|
| Source | 5,492,627,216 B |
| Source SHA-256 | `11d6047f...e2b` |
| Selected references / bases | 292 / 5,944,255,022 |
| Archive | 8,828,856,533 B (1.6074x source) |
| Archive SHA-256 | `f9966387...9066` |
| Directory entries / adaptive splits | 363,105 / 79 |
| Index / payload | 47,376,617 / 8,781,479,916 B |
| Payload pipeline | 258.354 s; 23,008,155 bp/s |
| Full construction including validation | 475.810 s |
| Whole command including both SHA-256 passes | 557.55 s |
| Archive validation | 202.109 s |
| First payload from command start | 64.679 s |
| Peak RSS | 8,776,260 KiB |
| Peak pending raw / compressed / total | 29,440,657 / 6,121,416 / 35,562,073 B |
| Occurrence index / payload spool / scratch | 0 / 0 / 0 B |

The post-build source-oracle query used four physical reads and fetched 294,190
bytes. Regional decode took 374.759 ms and the full local query, including
seven independent source-tile comparisons, took 2.273 seconds. Both
`correctness` and `haplotype_tiles_correct` were true.

The prior stopped writer measured 70,148 bp/s and projected 84,556 seconds
(23.49 hours). The completed command is 151.65x faster than that ETA;
construction including validation is 177.71x faster, while the payload pipeline
rate itself is 328.00x higher. The requirement was a real 100x improvement on
the large source, not a small-file extrapolation; the completed archive clears
that gate.

The external report used legacy `*_wall_ms` keys for three parallel worker sums.
Those values are aggregate worker milliseconds, not elapsed phase wall time.
The committed schema renames them to `*_worker_ms` and adds the actual bounded
payload-pipeline wall clock. The 258.354-second progress measurement above is
the authoritative elapsed pipeline value for this run.

## Remaining bottlenecks and limitations

- Upstream GBZ deserialization still requires the complete source and dominates
  the 8.37 GiB RSS. This work did not claim lazy or memory-mapped source access.
- Structural validation now costs 202 seconds because it rereads, decompresses,
  and checks every payload. It is the clearest next construction optimization,
  provided corruption coverage and fail-closed behavior remain equivalent.
- The archive is 1.607x the compressed GBZ. Layout/query benchmarks must decide
  whether faster codec settings, dictionaries, or record sharing improve size
  without harming independent range access.
- The TypeScript package decodes the current regional payload but does not yet
  open the archive bootstrap/directory over HTTP. No browser latency claim is
  made here.
