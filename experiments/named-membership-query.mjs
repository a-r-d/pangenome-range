import { FileRangeSource } from "../packages/browser/dist/node/index.js";
import { openPangenome } from "../packages/browser/dist/reader/index.js";

function fail(message) {
  throw new Error(`usage: ${message}`);
}

const [archivePath, sample, contig, startText, endText] = process.argv.slice(2);
if (!archivePath || !sample || !contig || !startText || !endText) {
  fail(
    "node experiments/named-membership-query.mjs ARCHIVE SAMPLE CONTIG START END",
  );
}
const start = Number(startText);
const end = Number(endText);
if (
  !Number.isSafeInteger(start) ||
  !Number.isSafeInteger(end) ||
  start >= end
) {
  fail("START and END must be a safe nonempty half-open interval");
}

async function openArchive() {
  const source = await FileRangeSource.open(archivePath);
  return { source, archive: await openPangenome(source) };
}

function requestRows(layer, trace) {
  return (trace?.requestRanges ?? []).map((range, index) => ({
    layer,
    index,
    offset: range.offset.toString(),
    length: range.length,
  }));
}

const graphHandle = await openArchive();
const graphStarted = performance.now();
const graph = await graphHandle.archive.query({
  sample,
  contig,
  start,
  end,
  trace: true,
});
const graphElapsedMs = performance.now() - graphStarted;
await graphHandle.archive.close();

const namedHandle = await openArchive();
const namedStarted = performance.now();
const named = await namedHandle.archive.queryWithPathMembership({
  sample,
  contig,
  start,
  end,
});
const namedElapsedMs = performance.now() - namedStarted;
const catalogInfo = await namedHandle.archive.pathCatalogInfo();
await namedHandle.archive.close();

const groups = named.pathMembership.tiles.flatMap((tile) => tile.groups);
const memberships = groups.flatMap((group) => group.memberships);
const occurrenceWeight = groups.reduce(
  (total, group) => total + group.occurrenceWeight,
  0n,
);
const membershipMultiplicity = memberships.reduce(
  (total, membership) => total + membership.multiplicity,
  0n,
);
const uniqueReferencedPaths = new Set(
  memberships.map((membership) => membership.pathId.toString()),
).size;
const uniqueSamples = new Set(
  named.pathMembership.paths.map((path) => path.sample),
).size;
const largestGroup = [...groups].sort((left, right) =>
  left.occurrenceWeight > right.occurrenceWeight
    ? -1
    : left.occurrenceWeight < right.occurrenceWeight
      ? 1
      : 0,
)[0];

const graphTrace = named.trace.graph;
const result = {
  archivePath,
  query: { sample, contig, start, end },
  catalog: {
    pathCount: catalogInfo.pathCount.toString(),
    identitySource: catalogInfo.identitySource,
    identitySourceSha256: catalogInfo.identitySourceSha256,
    membershipGroupCount: catalogInfo.membershipGroupCount.toString(),
    membershipOccurrenceTotal: catalogInfo.membershipOccurrenceTotal.toString(),
    membershipUniquePathTotal: catalogInfo.membershipUniquePathTotal.toString(),
    codecDistribution: {
      deltaGroups: catalogInfo.codecDistribution.deltaGroups.toString(),
      runGroups: catalogInfo.codecDistribution.runGroups.toString(),
    },
  },
  graphOnly: {
    wallMs: graphElapsedMs,
    tiles: graph.tiles.length,
    bytes: graph.trace?.totalBytes ?? 0,
    ranges: graph.trace?.requestRanges.length ?? 0,
    dependencyRounds: graph.trace?.dependencyRounds ?? 0,
    decodeMs: graph.trace?.decodeMs ?? 0,
    canonicalHash: graph.trace?.canonicalHash,
  },
  identityAware: {
    wallMs: namedElapsedMs,
    tiles: named.region.tiles.length,
    groups: groups.length,
    memberships: memberships.length,
    uniqueReferencedPaths,
    uniqueSamples,
    occurrenceWeight: occurrenceWeight.toString(),
    membershipMultiplicity: membershipMultiplicity.toString(),
    largestGroupOccurrenceWeight:
      largestGroup?.occurrenceWeight.toString() ?? "0",
    largestGroupUniquePathCount:
      largestGroup?.uniquePathCount.toString() ?? "0",
    graph: {
      bytes: graphTrace?.totalBytes ?? 0,
      ranges: graphTrace?.requestRanges.length ?? 0,
      dependencyRounds: graphTrace?.dependencyRounds ?? 0,
      decodeMs: graphTrace?.decodeMs ?? 0,
      canonicalHash: graphTrace?.canonicalHash,
    },
    membership: named.trace.membership,
    catalog: named.trace.catalog,
  },
  paths: named.pathMembership.paths.map((path) => ({
    pathId: path.pathId.toString(),
    rawName: path.canonicalName,
    sample: path.sample,
    contig: path.contig,
    haplotype: path.haplotype.toString(),
    fragment: path.fragment.toString(),
    sense: path.sense,
  })),
  requests: [
    ...requestRows("graph", graphTrace),
    ...requestRows("membership", named.trace.membership),
    ...requestRows("catalog", named.trace.catalog),
  ],
};

process.stdout.write(
  `${JSON.stringify(
    result,
    (_key, value) => (typeof value === "bigint" ? value.toString() : value),
    2,
  )}\n`,
);
