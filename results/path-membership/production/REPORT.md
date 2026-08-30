# Production named-path membership evidence

Date: 2026-08-29

Status: production implementation accepted locally; bounded scale evidence passed.

## What this proves

The normal Rust encoder can recover real GBWT source sequence IDs tile by tile,
preserve them through canonical traversal grouping, and write the registered
`path-members-v1-` extension without `gbwt-rs` or a global row-per-visit index.
The Rust validator reconstructs the anonymous graph payload and requires exact
group digest, occurrence-weight, membership, and multiplicity agreement. The
public TypeScript reader independently reads only selected membership and catalog
pages.

Both production-layout pilots ran with one encoder worker, a 64 MiB construction
queue, and a hard 4 GiB address-space limit. Neither used swap.

| Pilot | Tiles | Catalog paths | Groups | Memberships | Archive | Whole wall | Peak RSS |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Rice Xa7 | 1 | 104,959 | 51 | 60 | 352,420 B | 14.61 s | 177,528 KiB |
| HPRC TERT | 4 | 53,150 | 1,686 | 4,257 | 991,576 B | 99.08 s | 659,856 KiB |

The HPRC whole wall includes streaming the 5,492,627,216-byte source into
11,922,586,123 bytes of bounded ephemeral source-cache files and building the
compact path index. Named membership locate itself took 1,585.02 ms and observed
8,522 positions with at most 1,023 LF steps. This is a bounded interval result,
not an archive-wide construction estimate.

## Reader and validation evidence

Full Rust validation passed on both objects. HPRC reconciled all 1,686 groups and
4,257 memberships; rice reconciled all 51 groups and 60 memberships. The public
Node/TypeScript reader then queried the generated files through `FileRangeSource`:

| Pilot | Range reads | Bytes read | Referenced paths | Weight | Multiplicity |
| --- | ---: | ---: | ---: | ---: | ---: |
| Rice Xa7 | 8 | 22,573 | 44 | 60 | 60 |
| HPRC TERT | 9 | 89,818 | 464 | 4,257 | 4,257 |

The exact equality of occurrence weight and membership multiplicity is therefore
checked by both the Rust archive validator and the public reader result. The
checked-in golden archive separately covers exact catalog identity and corruption
rejection in automated Rust and TypeScript tests.

## Reproduction

Build the production binary:

```bash
cargo build --release -p pangenome-range-cli
```

The rice command was:

```bash
prlimit --as=4294967296 -- /usr/bin/time -v \
  target/release/pangenome-range encode chr06_mc.named.gbz rice.pngr \
  --sample NATELBORO --contig chr06 \
  --start 28868608 --end 28884992 \
  --window-size 16384 --max-chunks 1 \
  --threads 1 --max-queued-bytes 67108864 \
  --scratch-dir /tmp --path-membership --progress off
```

The HPRC command was:

```bash
prlimit --as=4294967296 -- /usr/bin/time -v \
  target/release/pangenome-range encode hprc-v2.1-mc-grch38.gbz hprc.pngr \
  --sample GRCh38 --contig chr5 \
  --start 1245184 --end 1310720 \
  --window-size 16384 --max-chunks 4 \
  --threads 1 --max-queued-bytes 67108864 \
  --scratch-dir /tmp --path-membership --progress plain
```

Full validation used one worker, a 512 MiB validation queue for HPRC, and a
384 MiB queue for rice. The queue size is an admission estimate, not resident
memory: observed validator peaks were 57,560 KiB and 14,456 KiB respectively.

The complete repository gates passed:

```bash
pnpm check
pnpm check:rust
```

## Explicit boundary

No whole-genome named-membership encode was launched. The 1000 Genomes pilot is
also explicitly excluded after the earlier HPRC `.ri` construction exhausted
host memory; these results make no claim about 1000G overhead, time, or memory.
The production path avoids that `.ri` builder, but a new population-scale run is
an operational rollout experiment and must retain hard memory, disk, interval,
and output guards.

Exact machine-readable values are retained in `summary.json`. Large GBZ inputs,
temporary source caches, generated `.pngr` files, and raw reports remain outside
the repository.
