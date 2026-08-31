# Chicken named-path demonstration

The default configured research demo opens a whole-reference-genome archive
derived from the published 30-assembly chicken graph at `bGalGal1b#chr15` near
**IGLL1**. Chromosome 15 is the starting locus; the remote object is a
1,498,984,132-byte `.pngr` archive.

The browser reads regional graph tiles with HTTP byte ranges, displays weighted
tile-local traversals, and lazily resolves exact GBWT source-path membership only
after a traversal is selected. The inspector can export the membership table as
TSV and reconstruct the selected tile-local sequence as FASTA. Neither export is
a complete assembly path.

## Validated published example

Rice et al. report an approximately 5 kb deletion relative to bGalGal1b in one
UCD312 haplotype ([DOI 10.1186/s12915-023-01758-0](https://doi.org/10.1186/s12915-023-01758-0)).
The checksum-pinned graph VCF contains the exact multiallelic record:

- chromosome and 1-based position: `chr15:7,944,313`;
- record ID: `>14904200>14904750`;
- reference allele length: 5,184 bases;
- first alternate traversal: direct edge `>14904200>14904750`;
- first alternate allele count: 1;
- UCD312 genotype: `12|1`.

The direct deletion edge crosses a 16 KiB archive boundary. It therefore maps to
two authoritative tile-local groups rather than one invented cross-tile path:

| Core tile | Traversal digest | Named source path |
| --- | --- | --- |
| 7,929,856–7,946,240 | `9a91efcf5ab825db78ea5a43597dd034` | `UCD312#2#h2tg000050l#fragment=1251366` |
| 7,946,240–7,962,624 | `59f15358db4a7b7f34f0aec13bc5c0b3` | `UCD312#2#h2tg000050l#fragment=1251366` |

Both groups have occurrence weight one and exact membership multiplicity one.
The catalog marks this row as a haplotype source path and says it is reverse
relative to each canonical displayed group.

The product keeps two claims visibly separate:

- **Published interpretation:** an approximately 5 kb deletion relative to
  bGalGal1b in one UCD312 haplotype.
- **Direct archive observation:** the exact source VCF deletion edge maps
  uniquely to these two tile-local groups and one named UCD312 GBWT path.

No breed, phenotype, ancestry, allele-frequency, or complete-chromosome claim is
derived from the path catalog. The preset is enabled only for archive SHA-256
`93bcd713ccda14bf4e650c1c8d56751e5ed5db7624aecbf76769fa1909d25e4e`.
`scripts/chicken/build-demo-presets.mjs` rechecks the VCF, archive, traversal
digests, and catalog; `data/chicken/demo-presets.json` is its retained output.
