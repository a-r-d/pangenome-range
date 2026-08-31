# PPanG rice Xa7 named-membership result

This is a bounded identity join against the unchanged v1 Xa7 archive tile
`NATELBORO chr06:28868608-28884992`. It is not a new archive format.

## Structure and correctness

The tile has 51 distinct anonymous traversal groups, 60 non-reference local
occurrences, and 44 unique source path IDs. Nine paths appear in multiple groups and
nine have more than one local occurrence. The groups are therefore not a disjoint
partition. The largest group carries 7/60 occurrences (11.67%). For every group, the
sum of named membership multiplicities exactly equals the existing anonymous weight.

The distributions are sparse: group carrier count is p50 1, p95 2, p99/max 7;
path-ID run length is 1 at every percentile; catalog density is p50 0.00000953 and max
0.0000667. This rejects set/complement as a general tile model on the first real
dataset.

The all-occurrence chromosome brute-force comparison did not complete because it was
running when the separate HPRC r-index job exhausted the host. The bounded tile's 122
start positions were recovered by the official locate oracle, but this report does
not relabel that narrower check as whole-chromosome equality.

## Codec and placement measurements

Across 51 groups, raw totals were: delta-varint 324 bytes, interval/run 435,
dense bitset 669,375, and Roaring-style array blocks 495. Complement was invalid.
After deterministic per-group tags, adaptive delta-varint is 375 bytes and wins all
51 groups. Release-mode reconstruction and encoding took 25.08 ms. Chromium decoded
the 799-byte framed corpus 1,000 times in 15.2 ms, or 0.0152 ms per corpus. The
research browser decoder is 138 source lines and supports delta, run, dense, and
Roaring-style tags.

The original exact global catalog variants are 9,500,586 bytes plain, 3,738,426
front-coded, and 3,748,710 columnar. They remain the retained whole-catalog baseline.
The new range-addressable prototype splits the same 104,959 records into independently
compressed, BLAKE3-authenticated pages. At the selected 1,024 records/page, its
complete catalog is 218,800 bytes: 103 zstd pages and a 5,008-byte header/directory.
Rust exhaustively reconstructed 104,959/104,959 records exactly. The Rust metadata
export also matched 104,959/104,959 normalized records from the independent C++
catalog exporter.

The Xa7 tile's 44 path IDs select five pages. With three merged payload ranges, a cold
catalog lookup uses four ranges, two dependency rounds, and 27,615 bytes: 5,008 root
bytes, 10,707 selected page bytes, and 11,900 bytes of deliberate gap overfetch. This
is 135.38 times smaller than fetching the old 3,738,426-byte catalog. Including the
145,864-byte graph query and 375-byte membership block, the revised cold identity
estimate is 173,854 bytes rather than 3,884,665 bytes. The complete same-object
extension estimate is 219,295 bytes including the 120-byte index, or 0.06734% over
the 325,664,519-byte archive. Once catalog pages are cached, the identity increment
is still only the 375-byte membership block.

Page size is a query tradeoff, not just a compression choice. Across 64-8,192 records
per page, the best tested designs cost 94,383 bytes at two total catalog ranges,
63,526 bytes at three, and 27,615 bytes at four. Chromium, Firefox, and WebKit each
made exactly the same four strict `206` requests and returned all 44 expected records
through independent JavaScript BLAKE3, zstd, and page decoding. Their repeated
loopback decode measurements were 15.484, 10.14, and 18.12 ms per 44-record query;
these are functional local measurements, not CDN latency.

The retained `.ri` is 1,054,507,727 bytes. Its original build took 28.40 s, peaked at
5,040,758,784 bytes RSS, and used no measured temporary bytes. One tile batch locate
took 1.30 s and peaked at 1,534,971,904 bytes RSS. Catalog extraction took 0.55 s at
475,852,800 bytes RSS.

The new Rust exporter, which uses the ordinary GBWT load and does not use the `.ri`,
produced the same normalized catalog in 0.33 s at 488,329,216 bytes peak RSS under a
4 GiB address-space cap. Building the selected paged object then took 145.47 ms under
a 1 GiB cap.

## Rust ordinary locate

The current Rust prototype uses the 11,323,784-byte document-array sample option
already in the 457,520,368-byte GBWT; it does not read or construct the 1.05 GB `.ri`.
The local `gbwt-rs` fork parses typed DA samples during the ordinary GBWT load, so the
BWT is loaded exactly once. Its output for all 122 tile starts is byte-for-byte
identical to the retained C++ output. The single-load run took 0.25 s wall, peaked at
488,169,472 bytes RSS, and used no swap under a 4 GiB address-space limit. The locate
phase itself took 11.64 ms; LF steps were p50 322, p99 985, and max 1,017.

A deterministic 4,096-position corpus (`seed=20260828`) also matched C++ byte for
byte. The single-load Rust path took 0.77 s wall and 488,407,040 bytes peak RSS,
versus 0.75 s and 1,535,127,552 bytes for C++ FastLocate with the existing `.ri`.
Rust's locate phase took 536.77 ms after a 217.39 ms load; LF steps were p50 497, p99
1,011, and max 1,023. Both processes had the same 4 GiB address-space limit. The
earlier retained `rust-locate.json` records the superseded two-pass adapter rather
than being overwritten; `rust-locate-single-pass.json` records the current path.
This is strong bounded chromosome evidence, not a whole-chromosome enumeration or
HPRC result.

## Xa7 comparison

The complete 16 kb tile touches 25 named biological accessions:
`ARC10497`, `Basmati`, `GOBOLSAIL`, `KHAOYAIGUANG`, `LIUXU`, `Nagina22`, `Sadri`,
`TG11`, `TG12`, `TG13`, `TG15`, `TG28`, `TG30`, `TG49`, `TG5`, `TG54`, `TG6`,
`TG61`, `TG62`, `TG70`, `TG81`, `WW8`, `wild111`, `wild219`, and `wild65`. It also
touches 19 generic `_gbwt_ref` path fragments. The dominant named traversal group has
7 unique accessions and 7 occurrences: `ARC10497`, `Basmati`, `Sadri`, `TG12`,
`TG54`, `TG81`, and `WW8`.

Neither 25 tile-wide carriers nor 7 carriers of one local pattern is definitionally
the PPanG paper's “16 of 113 genomes align to Xa7” count. This archive tile includes
flanks, generic fragments, alternate partial paths, and multiple traversal groups;
the current model does not yet identify which branch definition PPanG used for
“contains Xa7.” The experiment therefore reports both unique carriers and local
occurrences without forcing equality.
