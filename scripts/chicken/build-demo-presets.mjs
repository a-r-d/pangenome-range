#!/usr/bin/env node

import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { writeFile } from "node:fs/promises";
import { createInterface } from "node:readline";
import { createGunzip } from "node:zlib";
import { FileRangeSource } from "../../packages/browser/dist/node/index.js";
import { openPangenome } from "../../packages/browser/dist/reader/index.js";

const [archivePath, vcfPath, outputPath] = process.argv.slice(2);
if (archivePath === undefined || vcfPath === undefined) {
  console.error(
    "usage: build-demo-presets.mjs ARCHIVE.pngr pangenome.vcf.gz [OUTPUT.json]",
  );
  process.exit(2);
}

const expected = {
  archiveSha256:
    "93bcd713ccda14bf4e650c1c8d56751e5ed5db7624aecbf76769fa1909d25e4e",
  vcfBytes: 430_058_854,
  vcfMd5: "6f66845a728a43001dedc9661b1ffe49",
  vcfSha256: "ec85ea239c4d39cb77c09bfaca347347c73f8d54578a9ba4fb78cfa31eed5ee4",
  chrom: "chr15",
  position: 7_944_313,
  variantId: ">14904200>14904750",
  referenceLength: 5_184,
  directDeletionTraversal: ">14904200>14904750",
  carrier: "UCD312",
};

const [archiveIdentity, vcfIdentity] = await Promise.all([
  fileIdentity(archivePath, ["sha256"]),
  fileIdentity(vcfPath, ["md5", "sha256"]),
]);
assertEqual(archiveIdentity.sha256, expected.archiveSha256, "archive SHA-256");
assertEqual(vcfIdentity.bytes, expected.vcfBytes, "VCF byte length");
assertEqual(vcfIdentity.md5, expected.vcfMd5, "VCF MD5");
assertEqual(vcfIdentity.sha256, expected.vcfSha256, "VCF SHA-256");

const event = await readPublishedEvent(vcfPath);
const source = await FileRangeSource.open(archivePath);
const archive = await openPangenome({ source });
try {
  const query = {
    sample: "bGalGal1b",
    contig: "chr15",
    start: 7_929_856,
    end: 7_962_624,
    trace: true,
  };
  const result = await archive.queryWithPathMembership(query);
  const firstHandle = 14_904_200n << 1n;
  const secondHandle = 14_904_750n << 1n;
  const candidates = [];
  for (const tile of result.pathMembership.tiles) {
    const regionTile = result.region.tiles.find(
      (candidate) =>
        candidate.coreStart === tile.coreStart &&
        candidate.coreEnd === tile.coreEnd &&
        candidate.reference.sample === tile.reference.sample &&
        candidate.reference.contig === tile.reference.contig,
    );
    if (regionTile === undefined)
      throw new Error(`graph tile ${tile.coreStart}-${tile.coreEnd} is absent`);
    for (const group of tile.groups) {
      const nodes = group.orientedNodes;
      if (nodes === undefined) continue;
      const forward = containsPair(nodes, firstHandle, secondHandle);
      const reverse = containsPair(nodes, secondHandle | 1n, firstHandle | 1n);
      if (forward || reverse)
        candidates.push({ tile, regionTile, group, reverse });
    }
  }
  if (candidates.length === 0)
    throw new Error("published deletion traversal has no archive group");
  const pathIds = [
    ...new Set(
      candidates.flatMap(({ group }) =>
        group.memberships.map(({ pathId }) => pathId),
      ),
    ),
  ];
  const resolved = await archive.pathsByIds(pathIds, { trace: true });
  const pathById = new Map(
    pathIds.map((pathId, index) => [pathId, resolved[index]]),
  );
  const traversalGroups = candidates.map(({ tile, regionTile, group }) => ({
    tile: {
      sample: tile.reference.sample,
      contig: tile.reference.contig,
      start: tile.coreStart,
      end: tile.coreEnd,
      archiveOffset: regionTile.provenance.archiveOffset.toString(),
    },
    traversalDigest: bytesToHex(group.traversalDigest),
    occurrenceWeight: group.occurrenceWeight.toString(),
    memberships: group.memberships.map((membership) => {
      const path = pathById.get(membership.pathId);
      if (path === undefined)
        throw new Error(`path ${membership.pathId} is absent from the catalog`);
      return membershipRecord(path, membership);
    }),
  }));
  const memberships = traversalGroups.flatMap((group) => group.memberships);
  const uniqueMemberships = [
    ...new Map(memberships.map((item) => [item.pathId, item])).values(),
  ];
  const carrierMemberships = uniqueMemberships.filter(
    ({ sample }) => sample === expected.carrier,
  );
  if (carrierMemberships.length !== 1 || uniqueMemberships.length !== 1) {
    throw new Error(
      `published deletion groups resolved to ${uniqueMemberships.length} paths (${carrierMemberships.length} UCD312); expected one UCD312 path`,
    );
  }
  if (
    traversalGroups.some(
      ({ memberships: groupMemberships }) =>
        groupMemberships.length !== 1 ||
        groupMemberships[0]?.pathId !== carrierMemberships[0]?.pathId,
    )
  ) {
    throw new Error(
      "published deletion tile groups do not all bind exclusively to the same UCD312 path",
    );
  }

  function membershipRecord(path, membership) {
    return {
      pathId: path.pathId.toString(),
      canonicalName: path.canonicalName,
      sample: path.sample,
      contig: path.contig,
      haplotype: path.haplotype.toString(),
      fragment: path.fragment.toString(),
      sense: path.sense,
      multiplicity: membership.multiplicity.toString(),
      orientationRelativeToTraversal: membership.reversedRelativeToGroup
        ? "reverse"
        : "forward",
    };
  }

  const preset = {
    schemaVersion: 1,
    status: "validated",
    presets: [
      {
        id: "chicken-igll1-ucd312-deletion",
        archiveSha256: expected.archiveSha256,
        paperDoi: "10.1186/s12915-023-01758-0",
        sourceDatasetDoi: "10.5281/zenodo.10018222",
        sourceVcf: {
          filename: "pangenome.vcf.gz",
          bytes: expected.vcfBytes,
          md5: expected.vcfMd5,
          sha256: expected.vcfSha256,
          record: {
            chrom: event.chrom,
            position: event.position,
            id: event.id,
            referenceLength: event.referenceLength,
            alternateAlleleIndex: 1,
            alleleCount: event.alleleCount,
            ucd312Genotype: event.ucd312Genotype,
            graphTraversal: event.directTraversal,
          },
        },
        paperAnalysisRepository: {
          url: "https://github.com/WarrenLab/chicken-pangenome-paper",
          commit: "9c95d98ae51a1484b2c3a4c5867a4ac8b76d22b3",
        },
        region: query,
        traversalGroups,
        expectedSourcePaths: uniqueMemberships,
        publishedClaim:
          "Approximately 5 kb deletion relative to bGalGal1b found in one UCD312 haplotype.",
        observedArchiveEvidence:
          "The exact VCF deletion edge maps uniquely to these two adjacent tile-local traversal groups and one named UCD312 GBWT source path.",
        trace: {
          graph: result.trace.graph,
          membership: result.trace.membership,
          catalog: result.trace.catalog,
        },
      },
    ],
  };
  const json = JSON.stringify(preset, bigintJson, 2);
  if (outputPath === undefined) console.log(json);
  else await writeFile(outputPath, `${json}\n`);
} finally {
  await archive.close();
}

async function readPublishedEvent(path) {
  const input = createReadStream(path).pipe(createGunzip());
  const lines = createInterface({ input, crlfDelay: Infinity });
  let ucd312Column = -1;
  for await (const line of lines) {
    if (line.startsWith("#CHROM\t")) {
      ucd312Column = line.split("\t").indexOf(expected.carrier);
      continue;
    }
    if (line.startsWith("#")) continue;
    const fields = line.split("\t");
    if (
      fields[0] !== expected.chrom ||
      Number(fields[1]) !== expected.position ||
      fields[2] !== expected.variantId
    )
      continue;
    if (ucd312Column < 0)
      throw new Error("VCF header has no UCD312 sample column");
    const info = Object.fromEntries(
      fields[7].split(";").map((item) => {
        const separator = item.indexOf("=");
        return separator < 0
          ? [item, ""]
          : [item.slice(0, separator), item.slice(separator + 1)];
      }),
    );
    const directTraversal = info.AT?.split(",")[1];
    const alleleCount = Number(info.AC?.split(",")[0]);
    const genotype = fields[ucd312Column];
    assertEqual(fields[3].length, expected.referenceLength, "VCF REF length");
    assertEqual(
      directTraversal,
      expected.directDeletionTraversal,
      "VCF direct deletion traversal",
    );
    assertEqual(alleleCount, 1, "VCF deletion allele count");
    if (!genotype.split(/[|/]/).includes("1"))
      throw new Error(`UCD312 genotype ${genotype} does not carry allele 1`);
    lines.close();
    return {
      chrom: fields[0],
      position: Number(fields[1]),
      id: fields[2],
      referenceLength: fields[3].length,
      directTraversal,
      alleleCount,
      ucd312Genotype: genotype,
    };
  }
  throw new Error("exact published IGLL1 VCF event was not found");
}

async function fileIdentity(path, algorithms) {
  const hashes = Object.fromEntries(
    algorithms.map((algorithm) => [algorithm, createHash(algorithm)]),
  );
  let bytes = 0;
  for await (const chunk of createReadStream(path)) {
    bytes += chunk.length;
    for (const hash of Object.values(hashes)) hash.update(chunk);
  }
  return {
    bytes,
    ...Object.fromEntries(
      Object.entries(hashes).map(([name, hash]) => [name, hash.digest("hex")]),
    ),
  };
}

function containsPair(nodes, first, second) {
  for (let index = 0; index + 1 < nodes.length; index += 1) {
    if (nodes[index] === first && nodes[index + 1] === second) return true;
  }
  return false;
}

function bytesToHex(bytes) {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join(
    "",
  );
}

function assertEqual(actual, expectedValue, label) {
  if (actual !== expectedValue)
    throw new Error(`${label} is ${actual}; expected ${expectedValue}`);
}

function bigintJson(_key, value) {
  if (typeof value === "bigint") return value.toString();
  return value;
}
