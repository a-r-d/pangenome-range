# Default viewer indexes at whole-HPRC scale

## Verdict

Accepted. The default named-locus and multiscale-summary extensions add
241,958 bytes (0.00274%) to the exact same whole-HPRC source and archive
options. Whole wall was 503.36 seconds versus 499.34 seconds before the feature
change, a 4.02-second (0.81%) difference within ordinary run-to-run noise. Peak
RSS was 642,220 KiB (627.2 MiB), 34,160 KiB above the preceding disk-backed
run and still 92.68% below the fully loaded 8,775,928 KiB baseline.

The archive passed its mandatory pre-rename structural gate and the retained
nine-query independent GBZ source oracle. This closes the whole-source evidence
gap recorded by the bounded MHC feature pilot.

## Exact inputs and storage

The source was the retained 5,492,627,216-byte HPRC v2.1 Minigraph-Cactus
GRCh38 GBZ with SHA-256
`11d6047f79575ffb83757462484bad134ed20928bd2c8171ec52e35a54976e2b`.
The source, fresh output, and ephemeral source cache were on the separate
`/dev/nvme0n1` XFS volume; the repository and executable were on
`/dev/nvme1n1`. The run used disk-backed source access, eight workers, a
268,435,456-byte queue limit, 16,384 bp windows, zstd-3, and no annotation
file. The exact commands and release-binary checksum are in `config.json`.

## Size

| Measurement | Before viewer indexes | With viewer indexes | Delta |
| --- | ---: | ---: | ---: |
| Archive | 8,828,788,418 B | 8,829,030,376 B | +241,958 B (+0.00274%) |
| Bootstrap/index | 47,376,617 B | 47,376,777 B | +160 B |
| Regional plus extension bodies | 8,781,411,801 B | 8,781,653,599 B | +241,798 B |
| Directory pages | 11,559 | 11,559 | 0 |
| Physical chunks | 363,105 | 363,105 | 0 |

The 160-byte fixed cost is exactly two extension-directory entries. Extension
descriptors and child pages occupy 241,798 encoded bytes and 588,832 decoded
bytes. They contain 591 summary series and 8,017 summary bins. The named-locus
descriptor is valid but empty because this run deliberately supplied no GFF3;
the encoder never invents annotations or assembly identity.

The resulting archive SHA-256 is
`0b03325564dcfd558015f4722f509fd749216e30f98522c550182c1090317293`.
The SHA changes as expected because the extension directory and bodies are new;
the regional payload codec and semantic model did not change.

## Performance and memory

| Measurement | Disk-backed baseline | With viewer indexes | Delta |
| --- | ---: | ---: | ---: |
| Whole command | 499.34 s | 503.36 s | +4.02 s (+0.81%) |
| Prebuild | 95.034 s | 112.899 s | +17.865 s |
| Construction including validation | 369.561 s | 357.660 s | -11.902 s |
| Payload pipeline | 337.140 s | 328.715 s | -8.425 s |
| Pre-rename validation | 32.221 s | 28.743 s | -3.478 s |
| Output SHA-256 | 34.493 s | 32.557 s | -1.935 s |
| Peak RSS | 608,060 KiB | 642,220 KiB | +34,160 KiB |

These are single exact-source/options/host observations. The opposing prebuild
and payload movements show ordinary cache and scheduler variance, so this run
does not claim that the feature made encoding faster. The defensible result is
that no material whole-wall regression was observed. Feature finalization
itself took 188.823 ms. Peak queued raw-plus-compressed payload data was
35,562,074 bytes, well below the configured 256 MiB cap.

The ephemeral disk source cache was 11,921,858,427 bytes (11.10 GiB) and was
removed on exit. Occurrence-index, payload-spool, and general encoder scratch
remained zero. The completed 8.22 GiB archive remains outside the repository.

## Correctness

Before atomic rename, validation checked all 11,559 directory pages and all
363,105 physical payloads in 28.743 seconds. The retained oracle then passed
all 9/9 graph comparisons and all 58/58 selected tile-local weighted-haplotype
comparisons.

The oracle is a separate research process: it took 81.58 seconds and peaked at
12,973,152 KiB because it fully loads independent source structures. That RSS
is not part of the encoder's 642,220 KiB measurement and remains a verifier-side
scale problem, not an archive-construction requirement.

## Remaining limits

Whole-HPRC encoding with default summaries is now measured and accepted. Large
GFF3 annotation imports still sort searchable records in memory and need a
disk-backed external-sort pilot before multi-million-record annotation inputs
can be called bounded-memory. A whole 1000GP encode remains unauthorized until
the already-required bounded source/cache/index pilot establishes its resource
budget.
