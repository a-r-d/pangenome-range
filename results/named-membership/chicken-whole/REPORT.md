# Complete 30-chicken named-membership archive

The complete published chicken graph now has a production-shaped `.pngr` archive.
It covers all 207 `bGalGal1b` reference paths and 1,052,949,595 reference bases,
not just chromosome 15. The resulting immutable object is 1,498,984,132 bytes
with SHA-256
`93bcd713ccda14bf4e650c1c8d56751e5ed5db7624aecbf76769fa1909d25e4e`.

## Memory-safe source conversion

The original stock-vg whole-GFA attempt failed under a 12 GiB zero-swap cap
during path indexing. This run used the locally built vg containing the
GBWTGraph rvalue-move repair, closed other large desktop applications, limited
vg to one job with a 20-million-node buffer, and enforced `MemoryHigh=40G`,
`MemoryMax=42G`, and `MemorySwapMax=0`.

The complete 12,630,299,872-byte GFA converted to a 1,359,062,880-byte GBZ in
7 minutes 38 seconds. Peak RSS was 24,355,120 KiB and the process used no swap.
The GBZ reload check recovered 49,001,347 nodes, 12,237 paths, 18 samples, 30
haplotypes, and 6,188 contigs exactly.

## Archive construction

The encoder used the persistent disk-backed source cache, one worker, a 64 MiB
queue budget, direct embedded-DA locate, a hard 8,192 LF-step limit, and 25,437
mapped GRCg7b gene rows. It produced 64,371 independently compressed tiles in
14 minutes 11 seconds with 173,668 KiB process peak RSS and no swap. There was
no global occurrence index, payload spool, or scratch write.

The path catalog contains all 12,237 exact GBWT source paths. Across 717,130
canonical traversal groups, the archive stores 1,850,732 membership records and
an occurrence weight of 1,850,762. The difference is expected duplicate
multiplicity, not lost identity. The encoder located 3,830,242 positions and
needed at most 1,023 LF steps.

Writer finalization took 525 seconds, including 240 seconds of locate work. It
is the largest remaining construction-time cost, but it remained bounded in
memory.

## Correctness and publication

Independent full validation passed all 2,177 directory pages, 64,371 entries,
64,371 physical payloads, and four extensions. Eight source-oracle queries
cover chromosome starts and ends, chromosome interiors, chromosome Z, an
unlocalized scaffold, mitochondrion, and the biological `IGLL1` locus. Every
canonical hash and every tile-local haplotype comparison passed. The `IGLL1`
hash is unchanged from the chromosome 15 archive.

The content-addressed object is live at:

```text
https://archives.ard.ninja/pangenome-range/sha256/93bcd713ccda14bf4e650c1c8d56751e5ed5db7624aecbf76769fa1909d25e4e/chicken-whole-named.pngr
```

The retained origin check passed exact size and SHA-256, HEAD, byte ranges,
CORS and preflight, exposed headers, stable ETag, immutable no-transform cache
policy, and local/remote byte equality. The Pages chicken demo now defaults to
this whole-corpus archive. The older chromosome 15 object remains available as
historical bounded evidence.

The source is the Rice et al. 30-chicken pangenome graph, Zenodo record
10018222, licensed CC BY 4.0. Large GFA, GBZ, cache, annotation, and `.pngr`
objects remain outside the repository. The checked-in script requires the
measured move-fixed vg binary by checksum and preserves all memory and zero-swap
guards used for this run.
