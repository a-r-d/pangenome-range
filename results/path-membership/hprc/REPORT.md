# HPRC bounded named-membership result

The bounded biological workload was defined before running: HLA-B, MICB, KIR3DL1,
and TERT as a simpler control. The standalone GBWT extraction from the
5,492,627,216-byte HPRC source succeeded in 30.85 s, produced
4,051,134,848 bytes, and peaked at 13,922,924 KiB RSS.

The subsequent official `vg 1.76.1 gbwt -r` construction exhausted host memory and
left a zero-byte `.ri`. The host crash prevented `/usr/bin/time` from returning a
final peak or wall time, so neither is invented here. This is user-observed OOM
evidence; kernel-log confirmation was unavailable in the restricted environment.

After this failure, the original run produced no HPRC locate, membership, codec, or
overhead result. Those original empty CSVs remain absence markers rather than being
rewritten. The later bounded Rust results are retained separately in
`regional-membership.json` and `regional-tiles.csv`.

## Rust ordinary locate

The later local `gbwt-rs` single-load implementation made a bounded locate retry
possible without vg and without an `.ri`. It loaded the exact 4,051,134,848-byte GBWT
(`sha256:97e49520e93d343e0ba7b99952d9b8bf99a3aae6562ff1c4122488d38c8f01a7`)
once and decoded its 390,336,312-byte embedded DA option. A deterministic workload
initially selected 64 stored sequence IDs, walked forward at most 100,000 steps from
each known sequence start to produce an independent expected position-to-sequence
mapping, and then located those positions through LF. All 64 sequence IDs matched
exactly.

The capped process completed in 4.31 s wall at 4,919,664,640 bytes peak RSS and
reported zero swaps under a 12 GiB address-space limit. Source load took 3,883.61 ms,
the independent oracle walks took 192.09 ms, and the 64 locate calls took 6.80 ms.
LF steps were p50 204, p99/max 994.

Before the regional claim, the independent workload was increased to 1,024 known
sequence IDs with the same seed and forward-walk limit. All 1,024 sequence identities
matched. The process completed in 5.93 s at 4,920,123,392 bytes peak RSS with zero
swaps under the 12 GiB cap. This oracle is independent of the LF locator: expected
positions are constructed by walking from known source sequence starts.

## Paged path catalog

Catalog construction is now measured separately from regional membership. The Rust
exporter loaded the same 4,051,134,848-byte GBWT once under a 12 GiB address-space cap
and emitted all 53,150 path metadata records (106,300 stored sequences) in 2.44 s at
4,920,131,584 bytes peak RSS with zero swaps. The 13,225,188-byte NDJSON source has
SHA-256 `85e6d80f35d91f400559655e2c224eba9bec4bc7c9929bb9d3bcef2538fd14a9`.

All tested range-addressable layouts exhaustively decoded 53,150/53,150 records
exactly. Their complete sizes were 694,812 bytes at 1,024 records/page, 671,058 bytes
at 4,096, and 660,980 bytes at 8,192. The smallest is 0.00748329% of the existing
8,832,749,949-byte `.pngr`; its builder took 73.82 ms and 20,697,088 bytes peak RSS.
Pages and the directory carry BLAKE3-128 integrity and each page is independently
zstd-compressed.

A deliberately dispersed 64-path-ID stress query selected every page and therefore
read the complete 660,980 bytes. The actual biological path IDs are much more local
in canonical ID space, so the 1,024-record layout is better for these queries despite
its slightly larger complete catalog.

## Regional named memberships

Only the ten tiles overlapping the four declared intervals were decoded. They contain
15,936 traversal starts in total. Each region was located sequentially under a hard
12 GiB address-space cap; no `vg` command or `.ri` was used. Peak RSS stayed between
4,919,902,208 and 4,920,188,928 bytes and every process reported zero swaps. The
largest locate workload, TERT with 8,522 starts, took 1,476.19 ms after the ordinary
2,226.19 ms GBWT load.

| Region | Tiles | Named paths / samples | Groups | Membership | Catalog lookup | Graph + identity |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| HLA-B | 2 | 463 / 232 | 405 | 4,982 B | 31,232 B / 3 ranges | 214,040 B |
| MICB | 2 | 463 / 232 | 404 | 4,981 B | 31,232 B / 3 ranges | 138,879 B |
| KIR3DL1 | 2 | 465 / 233 | 359 | 7,548 B | 55,218 B / 4 ranges | 221,791 B |
| TERT | 4 | 464 / 232 | 1,686 | 21,768 B | 42,648 B / 4 ranges | 343,815 B |

For every traversal group, named membership multiplicities sum exactly to the existing
anonymous weight. HLA-B and MICB are disjoint within each source tile. One KIR3DL1
tile and two interior TERT tiles are not: paths appear in multiple groups and/or with
local multiplicity above one. This independently reproduces the rice conclusion that
a set of path IDs is not a sufficient general membership model.

HLA-B and MICB select the same 463 canonical paths: 462 haplotype paths plus the CHM13
reference path, spanning 232 sample labels. TERT selects 463 haplotypes plus CHM13.
KIR3DL1 selects 463 haplotypes plus CHM13 and GRCh38 reference-sense paths. These are
paths contributing tile-local traversal evidence, not a claim that every corresponding
sample contains a particular gene or allele. Anonymous evidence also remains scoped
to each source tile and is not stitched across tile boundaries.

The adaptive codec chose delta-varint for 2,853 of 2,854 groups and a 10-byte interval
run for one HLA-B group. The ten bounded tile memberships total 39,279 bytes. Combined
with the 694,812-byte paged catalog and a 624-byte experimental index, the bounded
extension components total 734,715 bytes, or 0.008318% of the base archive. This is
not an archive-wide membership-size projection.

Chromium, Firefox, and WebKit independently decoded every selected catalog record
through strict `206`, exact `Content-Range`, stable ETag, BLAKE3, and zstd checks. The
lookup cost was 31-55 KiB rather than the complete catalog. Exact request counts for
an integrated same-object extension remain unmeasured because `.pngr` v1 was not
changed.

## Correctness boundary

The regional memberships have three checks: exact agreement with every existing
anonymous weight, exact reconstruction of every selected paged-catalog record in Rust
and three browsers, and a 1,024/1,024 independent HPRC sequence-identity locate gate.
The exact 15,936 regional positions were not compared to C++ FastLocate because that
requires the HPRC `.ri` whose construction exhausted host memory. The Rust locator is
separately byte-identical to C++ on all synthetic occurrences, all rice tile starts,
and 4,096 deterministic rice positions. This boundary is retained rather than calling
the regional path IDs an independent C++ equality result.
