# Whole-HPRC GENCODE v50 named-locus benchmark

## Verdict

Accepted. GENCODE v50 comprehensive annotation on the GRCh38.p14 reference
chromosomes is the canonical named-locus release benchmark. The corrected
reference encoder represented all 78,733 GENCODE gene rows as 157,466
range-searchable records: one symbol and one Ensembl stable ID per gene. It did
not multiply gene labels across 11.16 million transcript and child-feature
rows.

The whole annotated archive completed in 420.848 seconds, remained below
700 MiB peak RSS, passed mandatory structural validation before atomic rename,
and added only 3,719,573 bytes (+0.0421%) to the same whole-HPRC archive. The
regional arithmetic index remained exactly 47,376,777 bytes.

## Input provenance

The graph is the retained 5,492,627,216-byte HPRC Release 2 Minigraph-Cactus
v2.1 GBZ anchored to GRCh38, SHA-256
`11d6047f79575ffb83757462484bad134ed20928bd2c8171ec52e35a54976e2b`.

The annotation is the official GENCODE Human Release 50 comprehensive GFF3 for
reference chromosomes only:

- URL: `https://ftp.ebi.ac.uk/pub/databases/gencode/Gencode_human/release_50/gencode.v50.annotation.gff3.gz`
- compressed: 160,590,749 bytes; official MD5
  `b9b1a3aa6aa0df7723303bf8f3f08717`; SHA-256
  `2aaf245c91ed00e80920953add6cfaffcccc876dc0aceeb6ca0c86d15875899a`
- uncompressed: 4,763,975,927 bytes; SHA-256
  `d97bc25b7d4d4aa9614c4cf6fa4748c083b1531cb60c52c0612c9ff7ec4813eb`

The archive embeds the uncompressed annotation SHA-256 and filename. GENCODE
uses `chr1`-style contigs; explicit `--annotation-sample GRCh38` binds those
coordinates to the archive's real `GRCh38` manifests without inventing or
rewriting PanSN identities.

## Pilot finding and repair

The first 6 Mbp chr6 pilot deliberately exercised the unmodified generic GFF3
importer. It proved sample/contig matching but exposed label amplification:

| Measurement | All GFF3 features | Gene-only policy |
| --- | ---: | ---: |
| Named-locus records | 279,428 | 1,130 |
| Pages | 602 | 5 |
| Encoded extension bytes | 2,079,877 | 29,557 |
| Decoded extension bytes | 43,278,083 | 167,224 |
| Exact `HLA-B` hits | 511 | 1 |
| Exact `MICA` hits | 190 | 1 |

GENCODE repeats `gene_name` and `gene_id` on transcript, exon, CDS, codon, and
UTR rows. Treating all of those rows as named loci made a simple gene search
return hundreds of child features. The reference encoder now selects only GFF3
feature type `gene`, while the v1 binary record remains generic. This is an
encoder policy correction, not a byte-format change.

## Whole-source measurement

This comparison uses the exact same source, archive options, host, and NVMe
layout as the accepted unified-worker baseline.

| Measurement | No annotations | GENCODE v50 | Delta |
| --- | ---: | ---: | ---: |
| Archive | 8,829,030,376 B | 8,832,749,949 B | +3,719,573 B (+0.0421%) |
| Regional index | 47,376,777 B | 47,376,777 B | 0 B |
| Physical chunks | 363,105 | 363,105 | 0 |
| Named-locus records | 0 | 157,466 | +157,466 |
| Named-locus pages | 0 | 612 | +612 |
| Whole command | 409.937 s | 420.848 s | +10.911 s (+2.66%) |
| Writer finalization | 0.280 s | 23.402 s | +23.122 s |
| Payload pipeline | 267.234 s | 261.548 s | -5.686 s |
| Validation | 30.454 s | 26.827 s | -3.627 s |
| Output SHA-256 | 32.701 s | 32.404 s | -0.297 s |
| Peak RSS | 640,556 KiB | 695,816 KiB | +55,260 KiB (+8.63%) |

The direct annotation cost is the 23.122-second increase in writer
finalization, which includes hashing and scanning the 4.76 GB GFF3, sorting the
157,466 selected records, and writing 612 compressed pages. The smaller
end-to-end delta reflects ordinary favorable movement in payload, validation,
and prebuild phases; it is not claimed as an annotation optimization.

`/usr/bin/time -v` independently observed 7:01.00 elapsed, 695,816 KiB peak
RSS, 461% average CPU, 18,952,205 voluntary context switches, and 203,441
involuntary context switches. Occurrence scratch, payload spool, and general
encoder scratch remained zero.

## Range-reader label checks

Independent rows were read from the GFF3 and compared with TypeScript
`searchLoci()` results from the completed static archive:

| Query | Result | Zero-based half-open interval | Strand | Cold bytes |
| --- | --- | --- | --- | ---: |
| `TERT` | `ENSG00000164362.23` | `chr5:1253146-1295086` | reverse | 69,044 |
| `HLA-B` | `ENSG00000234745.16` | `chr6:31353194-31367067` | reverse | 69,112 |
| `MICA` | `ENSG00000204520.16` | `chr6:31399783-31415315` | forward | 68,939 |
| `BRCA2` | `ENSG00000139618.19` | `chr13:32315085-32400268` | forward | 68,729 |
| `TP53` | `ENSG00000141510.21` | `chr17:7661778-7687546` | reverse | 68,086 |
| `BRCA1` | `ENSG00000012048.28` | `chr17:43044291-43170245` | reverse | 68,729 |
| `ENSG00000012048.28` | `BRCA1` | same interval | reverse | 68,987 |

Every exact query returned one untruncated `gene` hit. Cold searches required
two dependency rounds: the 61,145-byte descriptor and one 6.9-8.0 KiB leaf.
Prefix `BRCA` returned exactly BRCA1, BRCA1P1, and BRCA2 from one leaf. The
descriptor and leaves are byte-range reads; no regional graph payload or whole
object download is involved.

## Correctness

Standard validation checked all 11,559 directory pages, 363,105 physical
regional payloads, and the known extension structures before rename. The
embedded annotation digest equals the independently computed uncompressed GFF3
SHA-256.

Regional graph output was also compared with the prior byte-deterministic,
source-oracle-qualified whole archive at TERT, HLA-B, BRCA2, TP53, and BRCA1.
All 5/5 canonical graph hashes and all 23/23 tile-local weighted-haplotype
hashes matched. The annotation change therefore did not alter regional graph
or haplotype semantics.

## Remaining limitation

The importer still scans and hashes the full plain GFF3 and sorts selected gene
records in memory. GENCODE v50 demonstrates that this is practical: only
157,466 records and about 680 MiB total process peak on this run. Much larger or
non-gene annotation corpora would still require a bounded external sort or a
separate annotation-track design; they must not silently reuse this gene-locus
index as a general feature database.
