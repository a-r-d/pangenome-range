#!/usr/bin/env node
import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { FileRangeSource } from "../../packages/browser/dist/node/index.js";
import { openPangenome } from "../../packages/browser/dist/reader/index.js";

const [archiveArgument, evidenceArgument, workloadArgument] =
  process.argv.slice(2);
if (!archiveArgument || !evidenceArgument || !workloadArgument) {
  throw new Error("usage: query-xa7.mjs ARCHIVE EVIDENCE_JSON WORKLOAD_JSON");
}
const archivePath = resolve(archiveArgument);
const evidencePath = resolve(evidenceArgument);
const workloadPath = resolve(workloadArgument);

async function sha256(path) {
  const digest = createHash("sha256");
  for await (const chunk of createReadStream(path)) digest.update(chunk);
  return digest.digest("hex");
}

const archive = await openPangenome(await FileRangeSource.open(archivePath));
try {
  const loci = await archive.searchLoci({
    name: "Xa7",
    mode: "exact",
    trace: true,
  });
  if (loci.hits.length !== 1 || loci.hits[0].displayName !== "Xa7") {
    throw new Error(`expected exactly one Xa7 hit, found ${loci.hits.length}`);
  }
  const query = {
    id: "xa7-natelboro",
    class: "fixed-biological-xa7",
    sample: "NATELBORO",
    contig: "chr06",
    start: 28_873_554,
    end: 28_874_897,
    context: 100,
  };
  const result = await archive.query({ ...query, trace: true });
  if (!result.trace) throw new Error("Xa7 query did not return a trace");
  const archiveSha256 = await sha256(archivePath);
  const evidence = {
    schemaVersion: 1,
    archivePath,
    archiveSha256,
    locusSearch: loci,
    query,
    queryResult: {
      reference: result.graph.reference,
      tiles: result.tiles.length,
      nodes: result.graph.nodes.ids.length,
      edges: result.graph.edges.from.length,
      trace: result.trace,
    },
  };
  const stringify = (value) =>
    `${JSON.stringify(
      value,
      (_key, item) => (typeof item === "bigint" ? item.toString() : item),
      2,
    )}\n`;
  await writeFile(evidencePath, stringify(evidence));
  await writeFile(
    workloadPath,
    stringify({
      schemaVersion: 1,
      archiveSha256,
      seed: "xa7-curated",
      queries: [
        { ...query, expectedCanonicalHash: result.trace.canonicalHash },
      ],
    }),
  );
} finally {
  await archive.close();
}
