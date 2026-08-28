import { describe, expect, it } from "vitest";
import type { RegionQuery, RegionTile } from "../src/reader/types.js";
import {
  decideGraphRegion,
  recommendedGraphRegion,
} from "../src/viewer/browser-policy.js";
import { layoutTubeMap, nodeWidth } from "../src/viewer/tube-map-layout.js";
import {
  buildTubeMapModel,
  patternThickness,
} from "../src/viewer/tube-map-model.js";

const query: RegionQuery = {
  sample: "GRCh38",
  contig: "chr6",
  start: 100,
  end: 300,
};

describe("tube-map model", () => {
  it("is independent of tile arrival order and keeps patterns tile-local", () => {
    const left = makeTile(100, 0n, 0n, [40n, 16n, 10n, 4n]);
    const right = makeTile(200, 1_000n, 900n, [80n, 16n, 2n, 1n]);
    const project = (tiles: readonly RegionTile[]) => {
      const model = buildTubeMapModel(tiles, query, { maxPatterns: 4 });
      return {
        nodes: model.nodes.map((node) => node.key),
        edges: model.edges.map((edge) => edge.key),
        patterns: model.patterns.map((pattern) => ({
          id: pattern.id,
          tile: pattern.tileKey,
          weight: pattern.weight.toString(),
          keys: pattern.nodeKeys,
        })),
      };
    };

    expect(project([right, left])).toEqual(project([left, right]));
    expect(project([left, right]).patterns.map(({ id }) => id)).toEqual([
      "T2-P1",
      "T1-P1",
      "T1-P2",
      "T2-P2",
    ]);
    expect(
      project([left, right]).patterns.every(({ id, tile }) =>
        id.startsWith(`${tile}-`),
      ),
    ).toBe(true);
  });

  it("collapses maximal simple chains and expands an explicitly selected group", () => {
    const tile = makeLongChainTile();
    const collapsed = buildTubeMapModel([tile], query, {
      maxPatterns: 0,
      simplifyLinearChains: true,
    });
    const group = collapsed.nodes.find(
      (node) => (node.collapsedMembers?.length ?? 0) >= 3,
    );
    expect(group?.collapsedBaseLength).toBeGreaterThanOrEqual(3);
    if (group === undefined) throw new Error("fixture must collapse a chain");

    const expanded = buildTubeMapModel([tile], query, {
      maxPatterns: 0,
      simplifyLinearChains: true,
      expandedNodeGroups: [group.key],
    });
    expect(expanded.nodes.some((node) => node.key === group.key)).toBe(false);
    expect(expanded.nodes.length).toBeGreaterThan(collapsed.nodes.length);
  });

  it("refuses a misleading oversized display rather than truncating it", () => {
    const model = buildTubeMapModel([makeTile(100)], query, {
      simplifyLinearChains: false,
      maxDisplayedNodeGroups: 2,
      maxDisplayedTopologyEdges: 1,
    });
    expect(model.withinDisplayLimits).toBe(false);
    expect(model.displayLimitMessage).toMatch(/Zoom in/);
    expect(model.nodes.length).toBeGreaterThan(2);
  });

  it("bounds width and logarithmic pattern thickness", () => {
    expect(nodeWidth({ sequenceLength: 0 })).toBe(28);
    expect(nodeWidth({ sequenceLength: 1_000_000 })).toBe(140);
    expect(patternThickness(0n)).toBe(2);
    expect(patternThickness(1n)).toBe(2);
    expect(patternThickness(1_000_000n)).toBe(9);
  });
});

describe("tube-map layout", () => {
  it("keeps the reference ordered and separates deterministic alternate lanes", () => {
    const model = buildTubeMapModel([makeTile(100)], query, {
      maxPatterns: 4,
      simplifyLinearChains: false,
    });
    const first = layoutTubeMap(model, { width: 900, height: 500 });
    const second = layoutTubeMap(model, { width: 900, height: 500 });
    const reference = model.reference.map((key) =>
      first.nodes.find((node) => node.key === key),
    );
    expect(reference.every((node) => node?.lane === 0)).toBe(true);
    expect(reference.map((node) => node?.x)).toEqual(
      [...reference.map((node) => node?.x)].sort((a, b) => (a ?? 0) - (b ?? 0)),
    );
    expect(first.nodes.some((node) => node.lane !== 0)).toBe(true);
    expect(first).toEqual(second);
    expect(
      first.edges.some((edge) => edge.classification === "inversion"),
    ).toBe(true);
    const colocatedPatternLabels = first.patterns.flatMap((pattern, index) =>
      first.patterns
        .slice(index + 1)
        .flatMap((candidate) =>
          Math.abs(pattern.labelX - candidate.labelX) < 0.1 &&
          Math.sign(pattern.lane) === Math.sign(candidate.lane)
            ? [[pattern, candidate] as const]
            : [],
        ),
    );
    expect(colocatedPatternLabels.length).toBeGreaterThan(0);
    expect(
      colocatedPatternLabels.every(
        ([left, right]) => Math.abs(left.labelY - right.labelY) >= 12,
      ),
    ).toBe(true);
    expect(first.tileBoundaries[0]?.x).toBe(reference[0]?.x);
  });

  it("suppresses labels that cannot fit inside a collapsed node", () => {
    const model = buildTubeMapModel([makeLongChainTile()], query, {
      maxPatterns: 0,
      simplifyLinearChains: true,
    });
    const layout = layoutTubeMap(model, {
      width: 900,
      height: 500,
      zoom: 0.2,
    });
    const collapsed = layout.nodes.find(
      (node) => node.collapsedMembers !== undefined,
    );
    expect(collapsed).toBeDefined();
    expect(collapsed?.showLabel).toBe(false);
  });
});

describe("graph region policy", () => {
  const plan = {
    sample: "GRCh38",
    contig: "chr6",
    start: 100,
    end: 300,
    selectedChunks: 2,
    compressedBytes: 1_000n,
    decodedBytes: 4_000n,
    ranges: [],
  };

  it("checks exact directory-derived bytes and payloads", () => {
    expect(decideGraphRegion(query, plan)).toEqual({
      allowed: true,
      exceeded: [],
    });
    expect(
      decideGraphRegion(
        { ...query, end: 200_000 },
        { ...plan, selectedChunks: 9, compressedBytes: 5n * 1024n * 1024n },
      ),
    ).toEqual({
      allowed: false,
      exceeded: ["span", "compressedBytes", "payloads"],
    });
  });

  it("centers a bounded recommendation on the selected locus", () => {
    expect(
      recommendedGraphRegion(
        { ...query, start: 0, end: 200_000 },
        { start: 0, end: 1_000_000 },
        {
          reference: {
            sample: "GRCh38",
            contig: "chr6",
            start: 500_000,
            end: 504_000,
          },
        },
      ),
    ).toMatchObject({ start: 482_000, end: 522_000 });
  });
});

function makeTile(
  coreStart: number,
  archiveOffset = 0n,
  nodeShift = 0n,
  weights: readonly bigint[] = [40n, 4n, 10n, 4n],
): RegionTile {
  const ids = BigUint64Array.from([1n, 2n, 3n, 4n, 5n], (id) => id + nodeShift);
  const handleShift = nodeShift << 1n;
  const sequenceBytes = new TextEncoder().encode("ACGTTGCA");
  const sequenceOffsets = Uint32Array.from([0, 1, 2, 4, 6, 8]);
  const topology = {
    from: BigUint64Array.from(
      [2n, 4n, 6n, 2n, 8n, 6n],
      (id) => id + handleShift,
    ),
    to: BigUint64Array.from(
      [4n, 6n, 8n, 10n, 10n, 11n],
      (id) => id + handleShift,
    ),
  };
  const traversalOffsets = Uint32Array.from([0, 4, 7, 10, 13]);
  const orientedNodes = BigUint64Array.from(
    [2n, 4n, 6n, 8n, 2n, 8n, 10n, 2n, 4n, 6n, 6n, 11n, 10n],
    (id) => id + handleShift,
  );
  return regionTile({
    ids,
    sequenceOffsets,
    sequenceBytes,
    topology,
    traversalOffsets,
    orientedNodes,
    weights: BigUint64Array.from(weights),
    referenceTraversal: BigUint64Array.from(
      [2n, 4n, 6n, 8n],
      (id) => id + handleShift,
    ),
    coreStart,
    archiveOffset,
  });
}

function makeLongChainTile(): RegionTile {
  const ids = BigUint64Array.from([1n, 2n, 3n, 4n, 5n, 6n, 7n]);
  return regionTile({
    ids,
    sequenceOffsets: Uint32Array.from([0, 1, 2, 3, 4, 5, 6, 7]),
    sequenceBytes: new TextEncoder().encode("ACGTTGA"),
    topology: {
      from: BigUint64Array.from([2n, 4n, 6n, 8n, 10n, 12n]),
      to: BigUint64Array.from([4n, 6n, 8n, 10n, 12n, 14n]),
    },
    traversalOffsets: Uint32Array.from([0]),
    orientedNodes: BigUint64Array.from([]),
    weights: BigUint64Array.from([]),
    referenceTraversal: BigUint64Array.from([2n, 4n, 6n, 8n, 10n, 12n, 14n]),
    coreStart: 100,
    archiveOffset: 0n,
  });
}

function regionTile(input: {
  ids: BigUint64Array;
  sequenceOffsets: Uint32Array;
  sequenceBytes: Uint8Array;
  topology: { from: BigUint64Array; to: BigUint64Array };
  traversalOffsets: Uint32Array;
  orientedNodes: BigUint64Array;
  weights: BigUint64Array;
  referenceTraversal: BigUint64Array;
  coreStart: number;
  archiveOffset: bigint;
}): RegionTile {
  return {
    reference: { sample: "GRCh38", contig: "chr6", start: 0, end: 1_000 },
    coreStart: input.coreStart,
    coreEnd: input.coreStart + 100,
    start: input.coreStart,
    end: input.coreStart + 100,
    semantics: "anonymous-distinct-weighted-tile-paths",
    nodes: {
      ids: input.ids,
      sequenceOffsets: input.sequenceOffsets,
      sequenceBytes: input.sequenceBytes,
    },
    topology: input.topology,
    haplotypes: {
      kind: "weighted-traversals",
      traversalOffsets: input.traversalOffsets,
      orientedNodes: input.orientedNodes,
      weights: input.weights,
    },
    provenance: {
      archiveOffset: input.archiveOffset,
      compressedBytes: 120,
      uncompressedBytes: 420,
      codec: "zstd-3",
    },
    archiveOffset: input.archiveOffset,
    encodedLength: 120,
    nodeIds: input.ids,
    nodeSequenceOffsets: input.sequenceOffsets,
    nodeSequences: input.sequenceBytes,
    edges: BigUint64Array.from(
      Array.from(input.topology.from).flatMap((from, index) => {
        const to = input.topology.to[index];
        return to === undefined ? [] : [from, to];
      }),
    ),
    referenceTraversal: input.referenceTraversal,
    traversalOffsets: input.traversalOffsets,
    traversalNodes: input.orientedNodes,
    traversalWeights: input.weights,
  };
}
