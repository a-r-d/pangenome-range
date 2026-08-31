import { describe, expect, it } from "vitest";
import type {
  FeatureQueryTrace,
  NamedTraversalGroup,
  RegionTile,
} from "../src/reader/types.js";
import type { LocalPattern } from "../src/viewer/tube-map-model.js";
import {
  locateValidatedPresetGroups,
  matchValidatedPresetPatterns,
} from "../src/viewer/validated-preset.js";

const trace: FeatureQueryTrace = {
  dependencyRounds: 1,
  requestRanges: [],
  totalBytes: 10,
  cacheHits: 0,
  pagesAvoidedByLimit: 0,
  integrityMs: 0,
  decompressionMs: 0,
  decompressionTaskMs: 0,
  decodeMs: 0,
};

describe("validated published presets", () => {
  it.each([
    ["first", [tile(2)]],
    ["second", [tile(1)]],
  ])("rejects a missing %s group tile", async (_label, tiles) => {
    await expect(locate(tiles)).rejects.toMatchObject({
      code: "tile-count-mismatch",
    });
  });

  it("rejects a digest mismatch", async () => {
    await expect(
      locate([tile(1), tile(2)], (index) => group(index, { digest: 99 })),
    ).rejects.toMatchObject({ code: "digest-count-mismatch" });
  });

  it("rejects a wrong path ID", async () => {
    await expect(
      locate([tile(1), tile(2)], (index) => group(index, { pathId: 8n })),
    ).rejects.toMatchObject({ code: "membership-mismatch" });
  });

  it("rejects a wrong multiplicity", async () => {
    await expect(
      locate([tile(1), tile(2)], (index) => group(index, { multiplicity: 2n })),
    ).rejects.toMatchObject({ code: "membership-mismatch" });
  });

  it("rejects one rendered pattern for two expected groups", async () => {
    const result = await locate([tile(1), tile(2)]);
    expect(result.status).toBe("validated");
    if (result.status !== "validated") return;
    expect(() =>
      matchValidatedPresetPatterns(result.groups, [pattern(1)]),
    ).toThrowError(/tile offset 2 resolved to 0 displayed patterns/);
  });

  it("returns cancelled when the source changes during an async read", async () => {
    let current = true;
    const result = await locateValidatedPresetGroups({
      tiles: [tile(1), tile(2)],
      expectedGroups,
      isCurrent: () => current,
      loadMemberships: async (selected, recordTrace) => {
        recordTrace(trace);
        current = false;
        return [group(Number(selected.provenance.archiveOffset))];
      },
    });
    expect(result).toEqual({ status: "cancelled" });
  });

  it("requires and returns exactly two validated groups and patterns", async () => {
    const result = await locate([tile(1), tile(2)]);
    expect(result.status).toBe("validated");
    if (result.status !== "validated") return;
    expect(result.groups).toHaveLength(2);
    expect(result.traces).toHaveLength(2);
    expect(
      matchValidatedPresetPatterns(result.groups, [pattern(1), pattern(2)]),
    ).toHaveLength(2);
  });
});

const expectedGroups = [1, 2].map((index) => ({
  tile: {
    sample: "ref",
    contig: "chr1",
    start: index * 10,
    end: index * 10 + 10,
    archiveOffset: String(index),
  },
  traversalDigest: index.toString(16).padStart(2, "0"),
  occurrenceWeight: "1",
  memberships: [
    {
      pathId: "7",
      multiplicity: "1",
      orientationRelativeToTraversal: "reverse" as const,
    },
  ],
}));

async function locate(
  tiles: readonly RegionTile[],
  makeGroup: (index: number) => NamedTraversalGroup = (index) => group(index),
) {
  return locateValidatedPresetGroups({
    tiles,
    expectedGroups,
    isCurrent: () => true,
    loadMemberships: async (selected, recordTrace) => {
      recordTrace(trace);
      return [makeGroup(Number(selected.provenance.archiveOffset))];
    },
  });
}

function group(
  index: number,
  changes: {
    digest?: number;
    pathId?: bigint;
    multiplicity?: bigint;
  } = {},
): NamedTraversalGroup {
  return {
    traversalDigest: Uint8Array.of(changes.digest ?? index),
    occurrenceWeight: 1n,
    uniquePathCount: 1n,
    memberships: [
      {
        pathId: changes.pathId ?? 7n,
        multiplicity: changes.multiplicity ?? 1n,
        reversedRelativeToGroup: true,
      },
    ],
    orientedNodes: BigUint64Array.of(BigInt(index * 2)),
  };
}

function pattern(index: number): LocalPattern {
  return {
    id: `T${index}-P1`,
    tileKey: `T${index}`,
    tileStart: index * 10,
    tileEnd: index * 10 + 10,
    weight: 1n,
    orientedNodes: [BigInt(index * 2)],
    nodeKeys: [],
    source: {
      key: `T${index}`,
      coreStart: index * 10,
      coreEnd: index * 10 + 10,
      archiveOffset: BigInt(index),
      compressedBytes: 1,
      uncompressedBytes: 1,
    },
  };
}

function tile(index: number): RegionTile {
  const ids = BigUint64Array.of(BigInt(index));
  const sequenceOffsets = Uint32Array.of(0, 1);
  const sequenceBytes = Uint8Array.of(0x41);
  return {
    reference: { sample: "ref", contig: "chr1", start: 0, end: 100 },
    coreStart: index * 10,
    coreEnd: index * 10 + 10,
    start: index * 10,
    end: index * 10 + 10,
    semantics: "anonymous-distinct-weighted-tile-paths",
    nodes: { ids, sequenceOffsets, sequenceBytes },
    topology: { from: new BigUint64Array(), to: new BigUint64Array() },
    haplotypes: {
      kind: "weighted-traversals",
      traversalOffsets: Uint32Array.of(0),
      orientedNodes: new BigUint64Array(),
      weights: new BigUint64Array(),
    },
    provenance: {
      archiveOffset: BigInt(index),
      compressedBytes: 1,
      uncompressedBytes: 1,
      codec: "none",
    },
    archiveOffset: BigInt(index),
    encodedLength: 1,
    nodeIds: ids,
    nodeSequenceOffsets: sequenceOffsets,
    nodeSequences: sequenceBytes,
    edges: new BigUint64Array(),
    referenceTraversal: new BigUint64Array(),
    traversalOffsets: Uint32Array.of(0),
    traversalNodes: new BigUint64Array(),
    traversalWeights: new BigUint64Array(),
  };
}
