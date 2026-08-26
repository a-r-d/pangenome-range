# Upstream inventory

Snapshot inspected on 2026-08-25. Commits are research references, not Cargo
dependency pins unless explicitly listed in `Cargo.lock`.

| Project | Repository | Version / inspected commit | License | Purpose |
|---|---|---|---|---|
| GBZ-base | <https://github.com/jltsiren/gbz-base> | 0.6.1 / `a5ed1ff3ddc402e230d1187afa438e05c8b3654e` | MIT | SQLite local-query baseline, query semantics, fixture source |
| GBWT-rs / `gbz` | <https://github.com/jltsiren/gbwt-rs> | crate 0.7.0 / `a0d72bc3bd261fc433e59de64b2706f5f45708ad` | MIT | Rust GBZ parser and graph API |
| GBWTGraph | <https://github.com/jltsiren/gbwtgraph> | `e27bc439cf110ac8cca89fcecacda993d2c9df70` | MIT | GBZ v3 / GBWTGraph v4 serialization reference |
| GBWT | <https://github.com/jltsiren/gbwt> | `c2e0199694fe41ec46c61c201c6ae0cd7dd08783` | MIT | Compressed haplotype index implementation/serialization |
| vg | <https://github.com/vgteam/vg> | `ac5822f8d22a80df02f7c101aa90914d1bce0cfd` | MIT (repository; contrib/submodules vary) | Producer/tooling and fixture provenance |
| vg Snakemake | <https://github.com/vgteam/vg_snakemake> | `d938c2035fa5ce16acd69147743762b513292173` | GPL-3.0 | Medium MHC Minigraph-Cactus GBZ fixture and construction provenance |
| HPRC resources | <https://github.com/human-pangenomics/hpp_pangenome_resources> | `cd99ffb2fbddd2d5bd91f4483065a44bfe83b287` | No repository license found; HPRC Data Use Protocol applies | Current v2.1 object list and data caveats |
| PMTiles | <https://github.com/protomaps/PMTiles> | `182d5b3cfdc2f5a6adbc54630c612da2f6086bdd` | Spec CC0; reference implementations BSD-3-Clause | Range-oriented single-object directory analogy |
| COG spec | <https://github.com/cogeotiff/cog-spec> | `203241975d054e5c933493f65bc4810e93d0048a` | CC BY 4.0 | Ordered metadata/overview/block analogy |

## Cargo dependencies

Direct dependencies are constrained in the workspace manifest and exactly
resolved by `Cargo.lock`:

- `gbz = 0.7.0`
- `gbz-base = 0.6.1`
- `simple-sds = 0.4.2`
- `blake3 = 1.8.7` (resolved from the compatible `1.8.2` requirement)
- `serde = 1.0.229` and `serde_json = 1.0.151`
- `sha2 = 0.10.9`
- `zstd = 0.13.3`

GBZ 0.7.0 itself uses `simple-sds` 0.4.2 and `zstd` 0.13.3 in this lockfile.

## JavaScript benchmark dependencies

The private benchmark package uses `fzstd` 0.1.1 (MIT) through the public
reader as its default pure-JavaScript decoder, `@bokuweb/zstd-wasm` 0.0.27
(MIT) as the optional decoder comparison, and Playwright 1.62.1 (Apache-2.0)
for real-browser execution. The WASM package is benchmark-only and is not
exported by or selected as the default decoder in the public reader.

## Fixture provenance and terms

`micb-kir3dl1.gbz` is fetched from the pinned GBZ-base commit above. The
GBZ-base repository is MIT licensed; its README identifies the fixture as an
HPRC Minigraph-Cactus v1.1 subset also used by vg's haplotype-sampling tests.
The fixture is not committed here. The fetch script verifies SHA-256
`1d574ede7533150eb87f6837a7763d4eac120aa03f34877392ecdd53b0410788`.

Before distributing that binary or derived datasets, re-check the upstream
license/provenance and applicable HPRC Data Use Protocol. HPRC v2.1 resources in
particular are currently described upstream as not fully QC'd, unpublished, and
potentially containing known issues.

`mhc-10.gbz` is fetched from the pinned vg Snakemake commit above. Its upstream
README says it was built with Minigraph-Cactus from 10 haplotypes selected from
Heng Li's MHC-61 dataset, Zenodo DOI
<https://doi.org/10.5281/zenodo.6617246>. The fetch script verifies a size of
4,511,832 bytes and SHA-256
`a0b44236852d5659202a6855308020df05efd7c2be90645d341d94fb775df685`.
This small medium-scale fixture is intentionally committed for hermetic research
tests. Re-check the source record's license and HPRC data-use terms before
redistributing it or derived archives outside this repository.

## Literature

- Jouni Sirén and Benedict Paten, “GBZ file format for pangenome graphs,”
  *Bioinformatics* 38(22), 2022. <https://doi.org/10.1093/bioinformatics/btac656>
- Jouni Sirén, Benedict Paten, and the HPRC, “GBZ-base and GAF-base: Indexed
  pangenome file formats,” bioRxiv, 2026. This is a preprint and was not peer
  reviewed at the inspected version. <https://doi.org/10.64898/2026.07.10.737775>
- Jouni Sirén et al., “Pangenomics enables genotyping of known structural
  variants in 5,202 diverse genomes,” *Science* 374, 2021.
  <https://doi.org/10.1126/science.abg8871>
