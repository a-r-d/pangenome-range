#!/usr/bin/env node

import { FileRangeSource } from "../../packages/browser/dist/node/index.js";
import { openPangenome } from "../../packages/browser/dist/reader/index.js";

const [archivePath] = process.argv.slice(2);
if (archivePath === undefined) {
  console.error("usage: query-chicken.mjs ARCHIVE.pngr");
  process.exit(2);
}

const source = await FileRangeSource.open(archivePath);
const archive = await openPangenome({ source });

try {
  const locus = await archive.searchLoci({
    name: "IGLL1",
    mode: "exact",
    sample: "bGalGal1b",
    contig: "chr15",
    trace: true,
  });
  if (locus.hits.length !== 1) {
    throw new Error(`expected one IGLL1 hit, found ${locus.hits.length}`);
  }

  const query = {
    sample: "bGalGal1b",
    contig: "chr15",
    start: 7_913_472,
    end: 7_979_008,
    trace: true,
  };
  const combined = await archive.queryWithPathMembership(query);
  const catalog = await archive.pathCatalogInfo({ trace: true });
  const ucd312 = await archive.searchPaths({ sample: "UCD312", limit: 10 });

  let occurrenceWeight = 0n;
  let membershipMultiplicity = 0n;
  let groups = 0;
  let memberships = 0;
  for (const tile of combined.pathMembership.tiles) {
    groups += tile.groups.length;
    for (const group of tile.groups) {
      occurrenceWeight += group.occurrenceWeight;
      memberships += group.memberships.length;
      for (const membership of group.memberships) {
        membershipMultiplicity += membership.multiplicity;
      }
    }
  }
  if (occurrenceWeight !== membershipMultiplicity) {
    throw new Error(
      `membership multiplicity ${membershipMultiplicity} does not equal occurrence weight ${occurrenceWeight}`,
    );
  }

  const result = {
    archive: await archive.info(),
    locus: {
      annotationName: locus.annotationName,
      annotationSha256: locus.annotationSha256,
      totalIndexedRecords: locus.totalIndexedRecords,
      hit: locus.hits[0],
      trace: locus.trace,
    },
    query: {
      ...query,
      tiles: combined.region.tiles.length,
      nodes: combined.region.graph.nodes.ids.length,
      edges: combined.region.graph.edges.from.length,
      namedPathsReferenced: combined.pathMembership.paths.length,
      groups,
      memberships,
      occurrenceWeight,
      membershipMultiplicity,
      graphTrace: combined.trace.graph,
      membershipTrace: combined.trace.membership,
      catalogTrace: combined.trace.catalog,
    },
    catalog,
    ucd312Paths: ucd312.paths,
  };

  console.log(
    JSON.stringify(
      result,
      (_key, value) =>
        typeof value === "bigint"
          ? value.toString()
          : value instanceof Uint8Array || value instanceof BigUint64Array
            ? Array.from(value, String)
            : value,
      2,
    ),
  );
} finally {
  await archive.close();
}
