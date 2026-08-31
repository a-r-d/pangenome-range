# ADR: named path membership experiment

Date: 2026-08-28

Status: superseded by `ADR-path-membership-production.md`; retained experimental evidence

Branch: `experiment/named-path-membership`

## Decision scope

This experiment asks whether an unchanged v1 regional payload's anonymous local
traversal occurrences can be joined back to canonical GBWT path identities. It does
not change the normative `.pngr` format, production encoder, public TypeScript API,
npm package, or v1 fixtures.

The independent oracle is pinned to C++ GBWT
`c2e0199694fe41ec46c61c201c6ae0cd7dd08783` (1.6.0), vgteam SDSL
`a4c77d4ee040344ee8c0cd185b1d26a3c3a95436`, and vg 1.76.1. The pinned C++ API's
`FastLocate::decompressDA(node)` returns source sequence IDs for all offsets in an
oriented node record. Bidirectional sequence IDs are reduced with `Path::id()` while
the stored orientation is retained separately. The upstream references are the
[C++ GBWT repository](https://github.com/jltsiren/gbwt),
[gbwt-rs](https://github.com/jltsiren/gbwt-rs), and the
[vg GBWT documentation](https://github.com/vgteam/vg/wiki/VG-GBWT-Subcommand).

## Evidence

The synthetic fixture passes an exact 56-occurrence comparison among the checked-in
manual mapping, independent full-sequence enumeration, and FastLocate. It includes a
path that revisits nodes 2-3; that path occurs twice in both node records. The named
Rust copy carries start identity through canonical traversal grouping, subtracts one
real reference occurrence, and checks every membership multiplicity sum against the
unchanged production anonymous weight.

The rice Xa7 tile is decisive about semantics. It contains 51 groups, 60
non-reference occurrences, and 44 unique path IDs. Nine paths occur in multiple
groups and nine have local multiplicity above one. Memberships are not a disjoint
partition, so a plain `set<path_id>` and complement as a general default are wrong.

| Corpus | Paged catalog | Membership | Extension index | Extension overhead | Paged-catalog browser decode |
| --- | ---: | ---: | ---: | ---: | ---: |
| Synthetic | 408 B | 20 B | 120 B | 548 B / 10.81% | not run |
| Rice Xa7 tile | 218,800 B | 375 B | 120 B | 219,295 B / 0.06734% | 10.14-18.12 ms |
| HPRC four loci / 10 tiles | 694,812 B | 39,279 B | 624 B | 734,715 B / 0.008318% | 5.51-17.25 ms |
| 1000G | not measured | not measured | not measured | not measured | not measured |

The rice codec totals are delta-varint 324 bytes, interval/run 435, dense bitset
669,375, and Roaring-style blocks 495. Complement is invalid. Adaptive delta-varint
with one deterministic tag per group is 375 bytes and wins all 51 groups. The tiny
synthetic case instead chooses dense bitsets, which supports retaining adaptive
selection rather than hard-coding delta forever.

The HPRC `.ri` build remains important counter-evidence. Extracting its standalone
4,051,134,848-byte GBWT already peaked at 13,922,924 KiB RSS. `vg gbwt -r` then
exhausted host memory and produced no index. The larger 1000G pilot was correctly not
started after that gate failed. The HPRC bounded retry below avoids this failed path;
1000G membership and overhead remain unknown.

The preferred Rust mechanism now has a successful small-to-HPRC-scale prototype in
the local `a-r-d/gbwt-rs` fork on branch `experiment/da-samples-locate`. `GBWT` owns
typed `DASamples`, parses the `sampled_records`, `bwt_ranges`, `sampled_offsets`, and
sequence-ID vector during its ordinary single source load, and follows bounded LF
until a sample. It also retains the original serialized DA words; a regression test
proves that loading and reserializing an existing index preserves that option exactly.
The implementation does not load or build an `.ri` and does not create a
row-per-visit structure. All 56 synthetic occurrences match the manual table and C++
oracle. On rice, all 122 tile starts and a deterministic 4,096-position corpus are
byte-identical to C++ FastLocate output.

For the 4,096-position rice corpus, the single-load Rust path used 488,407,040 bytes
peak RSS and 0.77 s wall under a 4 GiB address-space limit. C++ plus the existing
`.ri` used 1,535,127,552 bytes and 0.75 s wall under the same limit. Rust's 4,096
locate calls took 536.77 ms after a 217.39 ms load; LF steps were p50 497, p99 1,011,
and max 1,023. The embedded DA option is 11,323,784 bytes, versus the
1,054,507,727-byte rice `.ri`. The earlier retained two-pass prototype remains useful
baseline evidence; it used 486,170,624 bytes and 0.79 s. These are bounded rice
results, not whole-chromosome claims.

The bounded HPRC scale gate also passes without vg or an `.ri`. In one load, Rust
opened the 4,051,134,848-byte GBWT and exactly recovered 64/64 deterministic sequence
identities whose expected positions were generated independently by forward walks
from known sequence starts. It completed in 4.31 s at 4,919,664,640 bytes peak RSS
under a 12 GiB address-space cap; max LF distance was 994. A stronger follow-up used
1,024 independently generated sequence identities and matched all 1,024 under the same
cap, with max LF distance 1,023.

The second missing construction piece, the catalog, now also has a bounded Rust and
browser prototype. The standalone experimental `PMPC0001` object uses a 64-byte
header, fixed 48-byte page directory entries, contiguous path-ID pages, page-local
front coding, adaptive zstd-3, 64-bit offsets, and BLAKE3-128 directory/page
integrity. The reader rejects truncation, trailing data, directory or payload
corruption, invalid dimensions, noncontiguous offsets, and unknown codecs. This is
not yet part of `.pngr`.

Rust catalog export exactly matched all 104,959 normalized C++ rice records. The
selected 1,024-record layout shrinks that catalog to 218,800 bytes. Exhaustive Rust
decode recovered 104,959/104,959 records, while the real Xa7 tile's 44 identities
needed a 5,008-byte root and three payload ranges: 27,615 bytes total in two
dependency rounds. Chromium, Firefox, and WebKit independently returned the same
44/44 records through four strict `206` requests with BLAKE3 verification and zstd
decode. Their loopback decode measurements are functional evidence, not internet
latency.

HPRC catalog export loaded the existing 4.05 GB GBWT once under the same 12 GiB cap,
completed in 2.44 s at 4,920,131,584 bytes peak RSS, and wrote 53,150 metadata records.
The smallest tested page layout is 660,980 bytes, or 0.007483% of the existing HPRC
archive, and exhaustively reconstructs 53,150/53,150 records. A deliberately dispersed
64-ID query touches every page. Actual region IDs are clustered: the 1,024-record
layout fetches 31,232 bytes for HLA-B/MICB, 55,218 for KIR3DL1, and 42,648 for TERT.

The four declared HPRC regions now pass bounded membership extraction without vg or
an `.ri`. Ten tiles contain 15,936 starts and 2,854 traversal groups. Every named
membership multiplicity sum equals the unchanged anonymous weight. Membership blocks
total 39,279 bytes. The 694,812-byte selected catalog plus a 624-byte ten-tile index
make the bounded extension components 734,715 bytes, or 0.008318% of the archive.
This is a ten-tile result, not an archive-wide membership projection.

HLA-B and MICB are disjoint within each tile, but one KIR3DL1 tile and two TERT tiles
contain paths in multiple groups and/or with multiplicity above one. HPRC therefore
confirms that a plain path-ID set is not a general membership representation. All
selected catalog records decode exactly in Chromium, Firefox, and WebKit. The exact
regional positions are not a C++ FastLocate equality corpus because producing the
required HPRC `.ri` is the retained OOM failure; the independent HPRC locate oracle is
the 1,024 known-sequence workload.

## Biological API result

The experimental `answer` command returns group carriers, node carriers, unique path
and sample counts, local occurrence counts, and all catalog metadata. The complete
rice tile touches 25 named accessions and 19 generic fragments; its dominant named
group has 7 unique paths and 7 occurrences. Neither quantity is comparable by
definition to PPanG's published 16/113 statement without reproducing its exact
alignment/containment rule. This experiment does not force the counts to agree.

## Placement assessment

Inline membership is smallest by 120 bytes in the rice model and adds no round, but
it changes the regional payload and forces graph-only callers to download identities.
A sidecar is experimentally easy but breaks the one-object model and requires strong
digest binding. The selected experimental placement is one optional same-object
`path-members-v1-` descriptor owning paged catalog and tile-membership children:
graph-only readers retain unchanged behavior, identity readers pay at most one extra
dependency round, and integrity binds both page sets to the base object. This type is
still deliberately absent from the normative v1 registry.

The former conservative cold rice estimate fetched the complete 3.74 MB catalog.
The measured paged design instead fetches 27,615 catalog bytes in four ranges. With
the existing 145,864 graph bytes and 375-byte membership block, the cold identity
query estimate becomes 173,854 bytes, 22.34 times smaller than the former 3,884,665
bytes. The complete same-object extension estimate becomes 219,295 bytes including
its 120-byte experimental index, or 0.06734% over the base rice archive. Once the
catalog root/pages are cached, the identity increment remains the 375-byte membership
block.

The page-size/range-budget tradeoff is measured rather than assumed. For the Xa7
identity set, the best tested layouts cost 94,383 bytes at two total catalog ranges,
63,526 bytes at three, and 27,615 bytes at four. The 1,024-record layout is therefore
the selected four-range point; the smallest complete catalog is not automatically the
best query layout.

## Native encoder options

1. The preferred Rust locate architecture now runs directly in
   `pangenome-range-build`. It parses only embedded DA support, uses the existing
   disk-backed records for bounded batched LF, and releases tile state before the next
   tile. The local fork remains an oracle and is not a workspace dependency.
2. Accepting an external `.ri` is the smallest experimental interface and is what
   produced the rice evidence. It is not viable as the default until construction is
   made safe: the first HPRC attempt OOMed the host.
3. An authenticated project-owned locate cache is viable only if it preserves the
   compressed sampled/run structure and binds to the source digest. A row per global
   path/node visit remains forbidden.
4. Bundling or invoking C++ would reuse proven APIs but complicates every native npm
   platform package. It remains the last resort.

## Decision

The same-object packaging and direct-construction gates now pass. The native encoder
can either accept the paired oracle files or generate the same catalog and membership
pages directly from the source GBZ. A four-tile HPRC TERT object contains 53,150
catalog records, 1,686 groups, and 4,257 memberships; direct construction located
8,522 starts in 1.487 seconds, used at most 1,023 LF steps, and peaked at 660,332 KiB
RSS with zero swaps under a 4 GiB limit.
Chromium, Firefox, and WebKit each recovered one tile's 464 exact records in four
strict range requests totaling 76,869 bytes.

The semantic model is feasible and the best observed membership representation is an
adaptive hybrid that chose delta-varint on rice. Same-object optional extensions are
the best provisional placement. Ordinary embedded-sample locate is now the leading
production extraction mechanism, and the paged catalog removes the former whole-
catalog query penalty. HPRC locate, catalog, and the four bounded biological-region
memberships now pass, as does bounded integrated same-object request topology.
The experiment's definition of done is satisfied except for the explicitly waived
1000G pilot. Persistent-cache DA retention, archive-wide HPRC construction cost, and
format/API standardization are separate productization work, not blockers to the
feasibility conclusion. Identity-aware stable `.pngr` publication remains paused;
the existing anonymous v1 format and publication path are unchanged.

EXPERIMENT_COMPLETE_WITH_1000G_EXCLUSION
