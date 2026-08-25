# HPRC v2.1 GRCh38 encoder scale run

Status: **intentionally aborted during unacceptable preprocessing**.

The 5,492,627,216-byte HPRC v2.1 Minigraph-Cactus GRCh38 GBZ loaded
successfully under a 18,000,000 KiB virtual-memory cap. The encoder then spent
47m27s building its temporary global path-occurrence table. At stop time:

- the temporary SQLite file was 157,105,246,208 bytes (146.32 GiB);
- temporary storage was already 28.60x the compressed source;
- encoder RSS was 8,417,564 KiB (8.03 GiB);
- the payload spool and final archive were still 0 bytes; and
- SQLite had not yet built the `visits_by_node` index.

This is a failed scale design, not a completed archive benchmark. No archive
size, construction time, or query result should be inferred from it.

The high RSS and huge temporary file have different causes. A standalone
source-only inspection peaked at 8,775,216 KiB while the upstream `gbz` object
was fully deserialized. The 157 GB disk expansion came from our
`PathOccurrenceIndex::build`, which materializes one SQLite row per path-node
visit before encoding any chunks.

The exact temporary SQLite file and empty payload spool were deleted after the
run was stopped. The source GBZ remains at:

```text
/media/ard/eba76579-d702-4ff0-b5dd-eb503a726a4d/pangenome-range-data/sources/hprc-v2.1-mc-grch38.gbz
```

See the [optimization log](../../docs/OPTIMIZATION_LOG.md) for the replacement
direction and acceptance gates.
