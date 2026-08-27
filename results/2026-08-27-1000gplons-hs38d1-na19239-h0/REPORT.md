# 1000 Genomes hs38d1 no-annotation archive

Verdict: **accepted** for publication as a static range-addressable research
archive.

This run encoded the canonical 1000 Genomes hs38d1 GBZ without annotations.
Because the source has no tagged reference sample, the real `NA19239`
haplotype-0 paths were selected explicitly as the archive coordinate anchor.
These are **not GRCh38 coordinates**, and the archive does not claim standard
reference or gene-coordinate semantics.

## Artifact

- Local archive:
  `/media/ard/eba76579-d702-4ff0-b5dd-eb503a726a4d/pangenome-range-data/runs/2026-08-27-1000gplons-hs38d1-na19239-h0-whole/1000gplons-hs38d1-na19239-h0-v1-t8-zstd3.pngr`
- Public archive:
  `https://archives.ard.ninja/pangenome-range/sha256/71730fab7aad0dbbef81cf7c74b4fa8dbacbb3aad5bab0a797349120b18f6afb/1000gplons-hs38d1-na19239-h0-v1-t8-zstd3.pngr`
- Bytes: 8,975,880,203
- SHA-256:
  `71730fab7aad0dbbef81cf7c74b4fa8dbacbb3aad5bab0a797349120b18f6afb`
- Public manifest:
  `https://archives.ard.ninja/pangenome-range/sha256/71730fab7aad0dbbef81cf7c74b4fa8dbacbb3aad5bab0a797349120b18f6afb/manifest.json`

The source GBZ was 17,771,541,912 bytes with SHA-256
`10518b259e8ba0b1a7fa586301a7dcea46e5152f53ee09a3e1ad7507efd98063`.
Its upstream directory was
`https://cgl.gi.ucsc.edu/data/giraffe/mapping/graphs/for-NA19239/1000gplons/hs38d1/`.

## Construction

The release encoder used the bounded disk-backed source path with eight
workers, a 256 MiB queue, 16,384 bp base windows, zstd level 3, and an exact
4 GiB cgroup memory ceiling. No annotation input, global occurrence index,
payload spool, or general scratch output was used.

| Measurement | Result |
| --- | ---: |
| Whole wall | 500.79 s |
| Source-cache build plus fused source SHA-256 | 102.864 s |
| Compact path index | 77.590 s |
| Time to first payload from encode start | 180.463 s |
| Archive construction including mandatory validation | 287.453 s |
| Process peak RSS | 586,276 KiB |
| Swaps | 0 |
| Reference paths / bases | 24 / 3,088,146,717 |
| Directory pages / entries | 5,901 / 190,958 |
| Physical payloads | 190,958 |
| Index bytes | 24,173,045 |
| Payload bytes | 8,951,707,158 |
| Named-locus records | 0 |

## Correctness

The mandatory pre-rename gate validated every directory entry and every
physical payload before atomic publication. A separately loaded GBZ oracle then
matched seven 32-65 kb queries covering chromosome 1 start and middle,
chromosomes 6 and 12 middle, chromosome 22 end, X middle, and Y end. All seven
canonical graph hashes and all 32 tile-local haplotype comparisons matched.
Exact queries and hashes are retained in `queries.csv`.

An optional exhaustive reconstruction run additionally checked a clean prefix
of 10,378 physical payloads with zero errors. It was intentionally stopped at
5.43% after measuring a further approximately 6.4-hour ETA; it is not presented
as a completed validation gate. The mandatory all-payload gate and independent
source-oracle workload are the acceptance evidence, matching the policy used
for the prior HPRC publication.

The public-origin probe passed HEAD, exact size and SHA-256, stable ETag,
identity encoding, `Accept-Ranges: bytes`, strict `206`/`Content-Range`, CORS
and preflight, exposed range headers, immutable/no-transform caching, and exact
local equality for four ranges including the final 32 bytes.

## Reproduction and retained raw evidence

Compact configuration and results are in `summary.json` and `queries.csv`.
Raw progress, timing, encode report, semantic-verification JSON, partial
full-validation evidence, manifest, and origin-check JSON remain outside the
repository under:

`/media/ard/eba76579-d702-4ff0-b5dd-eb503a726a4d/pangenome-range-data/runs/2026-08-27-1000gplons-hs38d1-na19239-h0-whole/`

The exact encoder binary in that directory is 6,660,280 bytes with SHA-256
`dfb093776b0459212129069aa179920bfe549eb1e1804e5ce8bae9ff228548d7`.
`pnpm check:rust` passed before the build.
