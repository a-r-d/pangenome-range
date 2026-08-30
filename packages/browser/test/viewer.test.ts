import { describe, expect, it, vi } from "vitest";
import type {
  PangenomeArchive,
  QueryTrace,
  RegionQuery,
  RegionTile,
} from "../src/reader/types.js";
import { transformedRegion } from "../src/viewer/controller.js";
import {
  chooseViewerLod,
  DEFAULT_COMPLEXITY_BUDGETS,
  recommendedSummaryBins,
} from "../src/viewer/lod.js";
import {
  hitTestNode,
  layoutViewerModel,
  ViewerModelBuilder,
  viewerBudgets,
  viewerSummary,
} from "../src/viewer/model.js";
import {
  formatGenomicCoordinate,
  parseGenomicCommand,
} from "../src/viewer/navigation.js";
import { ProgressiveTileQuery } from "../src/viewer/query-controller.js";
import {
  renderViewerCanvas,
  shouldAggregateTopology,
} from "../src/viewer/renderer.js";

const query: RegionQuery = {
  sample: "GRCh38",
  contig: "chr1",
  start: 100,
  end: 200,
  context: 20,
};

describe("viewer model and layout", () => {
  it("prioritizes reference nodes and enforces every rendering budget", () => {
    const builder = new ViewerModelBuilder(
      query,
      viewerBudgets({
        maxRenderedNodes: 2,
        maxRenderedEdges: 1,
        maxHaplotypeLanes: 1,
      }),
    );
    builder.addTile(makeTile(0n));
    const model = builder.snapshot();

    expect([...model.nodes.keys()]).toEqual([1n, 2n]);
    expect(model.edges).toHaveLength(1);
    expect(model.traversals).toHaveLength(1);
    expect(model.traversals[0]?.tileStart).toBe(100);
    expect(model.counts).toMatchObject({
      decodedNodes: 4,
      renderedNodes: 2,
      decodedEdges: 3,
      renderedEdges: 1,
      decodedTraversals: 2,
      renderedHaplotypeLanes: 1,
    });
    expect(viewerSummary(model.counts)).toContain("Rendering budget reached");
  });

  it("lays out reference and alternate branches separately and supports hit tests", () => {
    const builder = new ViewerModelBuilder(
      query,
      viewerBudgets({
        maxRenderedNodes: 4,
        maxRenderedEdges: 4,
        maxHaplotypeLanes: 3,
      }),
    );
    builder.addTile(makeTile(0n));
    const layout = layoutViewerModel(builder.snapshot(), {
      width: 900,
      height: 460,
      zoom: 2,
      panX: -25,
    });
    const reference = layout.nodes.find((node) => node.id === 1n);
    const alternate = layout.nodes.find((node) => node.id === 3n);

    expect(reference?.reference).toBe(true);
    expect(alternate?.reference).toBe(false);
    expect(alternate?.y).not.toBe(reference?.y);
    expect(layout.edges.some((edge) => !edge.reference)).toBe(true);
    expect(layout.traversals).toHaveLength(2);
    expect(layout.tileBoundaries[0]?.archiveOffset).toBe(0n);
    expect(reference).toBeDefined();
    expect(
      hitTestNode(layout, (reference?.x ?? 0) + 1, (reference?.y ?? 0) + 1)?.id,
    ).toBe(1n);
  });

  it("produces a stable graph layout as progressive tiles arrive", () => {
    const first = new ViewerModelBuilder(query, viewerBudgets());
    first.addTile(makeTile(900n, 10n, 200));
    first.addTile(makeTile(100n, 0n, 100));
    const second = new ViewerModelBuilder(query, viewerBudgets());
    second.addTile(makeTile(100n, 0n, 100));
    second.addTile(makeTile(900n, 10n, 200));
    const project = (builder: ViewerModelBuilder) =>
      layoutViewerModel(builder.snapshot(), {
        width: 900,
        height: 460,
      }).nodes.map(
        ({ id, x, y, lane, branchKind, anchorStart, anchorEnd }) => ({
          id: id.toString(),
          x,
          y,
          lane,
          branchKind,
          anchorStart,
          anchorEnd,
        }),
      );

    expect(project(first)).toEqual(project(second));
  });

  it("renders the bounded layout through Canvas 2D", () => {
    const builder = new ViewerModelBuilder(query, viewerBudgets());
    builder.addTile(makeTile(0n));
    const layout = layoutViewerModel(builder.snapshot(), {
      width: 900,
      height: 460,
    });
    const context = fakeCanvasContext();

    renderViewerCanvas(context.value, layout, { selectedNodeId: 1n });

    expect(context.clearRect).toHaveBeenCalledOnce();
    expect(context.fillRect).toHaveBeenCalled();
    expect(context.quadraticCurveTo).toHaveBeenCalled();
    expect(context.fillText).toHaveBeenCalledWith(
      expect.stringContaining("GRCh38"),
      expect.any(Number),
      expect.any(Number),
    );
  });

  it("aggregates dense topology at base resolution", () => {
    const builder = new ViewerModelBuilder(query, viewerBudgets());
    builder.addTile(makeTile(0n));
    const layout = layoutViewerModel(builder.snapshot(), {
      width: 900,
      height: 460,
    });
    const alternateNode = layout.nodes.find((node) => !node.reference);
    const alternateEdge = layout.edges.find((edge) => !edge.reference);
    if (alternateNode === undefined || alternateEdge === undefined) {
      throw new Error("viewer fixture must contain alternate topology");
    }
    const nodes = Array.from({ length: 600 }, (_, index) => ({
      ...alternateNode,
      id: BigInt(index + 10),
      x: 60 + (index % 300) * 2.5,
      visible: true,
    }));
    const edges = Array.from({ length: 1_200 }, (_, index) => ({
      ...alternateEdge,
      from: BigInt(index + 10),
      to: BigInt(index + 11),
      fromX: 60 + (index % 180) * 4,
      toX: 120 + (index % 180) * 4,
    }));
    const denseLayout = {
      ...layout,
      query: { ...layout.query, start: 150, end: 151 },
      nodes,
      edges,
    };
    const context = fakeCanvasContext();

    expect(shouldAggregateTopology(denseLayout)).toBe(true);
    renderViewerCanvas(context.value, denseLayout, { displayMode: "base" });

    expect(context.quadraticCurveTo).not.toHaveBeenCalled();
    expect(context.bezierCurveTo.mock.calls.length).toBeLessThanOrEqual(10);
    expect(context.fillText).toHaveBeenCalledWith(
      "1 bp focus · 100 bp source-tile context",
      expect.any(Number),
      64,
    );
  });
});

describe("genomic command parsing", () => {
  const references = [
    { sample: "GRCh38", contig: "chr6", start: 0, end: 200_000_000 },
    { sample: "CHM13", contig: "chr1", start: 0, end: 248_000_000 },
  ];

  it.each([
    ["chr6:31,498,145-31,511,124", "GRCh38", "chr6", 31_498_145, 31_511_124],
    ["GRCh38 chr6:31498145-31511124", "GRCh38", "chr6", 31_498_145, 31_511_124],
    ["CHM13#chr1:1000000-1100000", "CHM13", "chr1", 1_000_000, 1_100_000],
  ])(
    "resolves %s against real archive references",
    (input, sample, contig, start, end) => {
      const parsed = parseGenomicCommand(input as string, references, "GRCh38");
      expect(parsed).toMatchObject({ kind: "coordinate", start, end });
      if (parsed.kind === "coordinate") {
        expect(parsed.reference).toMatchObject({ sample, contig });
        expect(parsed.canonical).toBe(
          formatGenomicCoordinate(
            sample as string,
            contig as string,
            start as number,
            end as number,
          ),
        );
      }
    },
  );

  it("routes names to the archive-native locus index", () => {
    expect(parseGenomicCommand("HLA-B", references)).toEqual({
      kind: "locus",
      name: "HLA-B",
    });
  });

  it("rejects ambiguous and absent coordinate references", () => {
    const ambiguous = [
      ...references,
      { sample: "CHM13", contig: "chr6", start: 0, end: 200_000_000 },
    ];
    expect(() => parseGenomicCommand("chr6:10-20", ambiguous)).toThrow(
      /multiple reference samples/,
    );
    expect(() => parseGenomicCommand("chr99:10-20", references)).toThrow(
      /no overlapping reference/,
    );
  });
});

describe("viewer viewport transforms", () => {
  it("resolves a two-base zoom to a one-base integer interval", () => {
    expect(
      transformedRegion({ ...query, start: 100, end: 102 }, 2.11, 0, 800),
    ).toMatchObject({ start: 100, end: 101 });
  });
});

describe("summary-driven detail policy", () => {
  const reference = {
    sample: "GRCh38",
    contig: "chr6",
    start: 0,
    end: 100_000,
  };
  const bin = {
    reference,
    fullBinStart: 0,
    fullBinEnd: 100_000,
    coverageFraction: 1,
    level: 0,
    binSpan: 10_000,
    coveredBases: 10_000n,
    tileCount: 1n,
    encodedBytes: 10_000n,
    decodedBytes: 100_000n,
    nodeRecords: 500n,
    edgeRecords: 700n,
    gbwtRecords: 1_000n,
    occurrences: 5_000n,
  };

  it("loads detail only when scale and every complexity counter fit", () => {
    const decision = chooseViewerLod([bin], { start: 0, end: 100_000 }, 1_000);
    expect(decision.mode).toBe("detailed");
    expect(decision.automaticDetail).toBe(true);
    expect(decision.limitingMetrics).toEqual([]);
  });

  it("declines detail when an archive-derived budget is exceeded", () => {
    const decision = chooseViewerLod(
      [{ ...bin, decodedBytes: 100_000_000n, occurrences: 3_000_000n }],
      { start: 0, end: 100_000 },
      1_000,
    );
    expect(decision.mode).toBe("regional");
    expect(decision.automaticDetail).toBe(false);
    expect(decision.limitingMetrics).toEqual(["decodedBytes", "occurrences"]);
  });

  it("uses exact directory transport and coverage-prorated record estimates", () => {
    const decision = chooseViewerLod(
      [
        {
          ...bin,
          reference: { ...reference, start: 40_000, end: 50_000 },
          coverageFraction: 0.1,
          decodedBytes: 100_000_000n,
          occurrences: 3_000_000n,
        },
      ],
      { start: 40_000, end: 50_000 },
      1_000,
      DEFAULT_COMPLEXITY_BUDGETS,
      false,
      { compressedBytes: 50_000n, decodedBytes: 500_000n },
    );
    expect(decision.estimates.decodedBytes).toBe(500_000n);
    expect(decision.estimates.occurrences).toBe(300_000n);
    expect(decision.usesPartialBinEstimates).toBe(true);
    expect(decision.automaticDetail).toBe(true);
  });

  it("requests approximately one bin per four horizontal pixels", () => {
    expect(recommendedSummaryBins(1_200)).toBe(300);
    expect(recommendedSummaryBins(100)).toBe(32);
  });
});

describe("progressive viewer queries", () => {
  it("cancels a stale query before it can append a tile or trace", async () => {
    const archive = delayedArchive();
    const controller = new ProgressiveTileQuery(archive);
    const tiles: bigint[] = [];
    const traces: string[] = [];
    const first = controller.run(
      { ...query, start: 100, end: 110 },
      {
        onTile: (tile) => tiles.push(tile.archiveOffset),
        onTrace: (trace) => traces.push(trace.canonicalHash),
      },
    );
    const second = controller.run(
      { ...query, start: 120, end: 130 },
      {
        onTile: (tile) => tiles.push(tile.archiveOffset),
        onTrace: (trace) => traces.push(trace.canonicalHash),
      },
    );

    await Promise.all([first, second]);
    expect(tiles).toEqual([120n]);
    expect(traces).toEqual(["120"]);
    controller.destroy();
  });
});

function makeTile(
  archiveOffset: bigint,
  nodeShift = 0n,
  coreStart = 100,
): RegionTile {
  const sequenceBytes = new TextEncoder().encode("ACGTT");
  const ids = BigUint64Array.from([1n, 2n, 3n, 4n], (id) => id + nodeShift);
  const sequenceOffsets = Uint32Array.from([0, 1, 2, 4, 5]);
  const handleShift = nodeShift << 1n;
  const edgeFrom = BigUint64Array.from([2n, 6n, 6n], (id) => id + handleShift);
  const edgeTo = BigUint64Array.from([4n, 4n, 8n], (id) => id + handleShift);
  const referenceTraversal = BigUint64Array.from(
    [2n, 4n],
    (id) => id + handleShift,
  );
  const traversalOffsets = Uint32Array.from([0, 2, 5]);
  const traversalNodes = BigUint64Array.from(
    [2n, 4n, 2n, 6n, 4n],
    (id) => id + handleShift,
  );
  const traversalWeights = BigUint64Array.from([10n, 3n]);
  return {
    reference: { sample: "GRCh38", contig: "chr1", start: 100, end: 200 },
    coreStart,
    coreEnd: coreStart + 100,
    start: coreStart,
    end: coreStart + 100,
    semantics: "anonymous-distinct-weighted-tile-paths",
    nodes: { ids, sequenceOffsets, sequenceBytes },
    topology: { from: edgeFrom, to: edgeTo },
    haplotypes: {
      kind: "weighted-traversals",
      traversalOffsets,
      orientedNodes: traversalNodes,
      weights: traversalWeights,
    },
    provenance: {
      archiveOffset,
      compressedBytes: 100,
      uncompressedBytes: 200,
      codec: "zstd-3",
    },
    archiveOffset,
    encodedLength: 100,
    nodeIds: ids,
    nodeSequenceOffsets: sequenceOffsets,
    nodeSequences: sequenceBytes,
    edges: BigUint64Array.from(
      [2n, 4n, 6n, 4n, 6n, 8n],
      (id) => id + handleShift,
    ),
    referenceTraversal,
    traversalOffsets,
    traversalNodes,
    traversalWeights,
  };
}

function delayedArchive(): PangenomeArchive {
  return {
    formatVersion: 1,
    semantics: "anonymous-distinct-weighted-tile-paths",
    references: () => [],
    capabilities: () => ({
      namedLoci: false,
      multiscaleSummaries: false,
      pathMembership: false,
    }),
    info: async () => ({
      formatVersion: 1,
      haplotypeSemantics: "anonymous-distinct-weighted-tile-paths",
      archiveBytes: 0n,
      references: [],
      extensions: [],
      namedLoci: { state: "absent", recordCount: 0n },
      pathMembership: { state: "absent", pathCount: 0n },
    }),
    searchLoci: async () => {
      throw new Error("not implemented by viewer test archive");
    },
    summary: async () => {
      throw new Error("not implemented by viewer test archive");
    },
    pathCatalogInfo: async () => {
      throw new Error("not implemented by viewer test archive");
    },
    pathById: async () => {
      throw new Error("not implemented by viewer test archive");
    },
    searchPaths: async () => {
      throw new Error("not implemented by viewer test archive");
    },
    tilePathMemberships: async () => {
      throw new Error("not implemented by viewer test archive");
    },
    pathMembership: async () => {
      throw new Error("not implemented by viewer test archive");
    },
    queryWithPathMembership: async () => {
      throw new Error("not implemented by viewer test archive");
    },
    planRegion: async (region) => ({
      sample: region.sample,
      contig: region.contig,
      start: region.start,
      end: region.end,
      selectedChunks: 0,
      compressedBytes: 0n,
      decodedBytes: 0n,
      ranges: [],
    }),
    query: async () => {
      throw new Error("not used");
    },
    queryTiles: async function* (region) {
      await delay(region.start === 100 ? 25 : 0);
      region.signal?.throwIfAborted();
      yield makeTile(BigInt(region.start));
      if (typeof region.trace === "function")
        region.trace(makeTrace(region.start));
    },
    cacheStats: () => ({
      directoryBytes: 0,
      directoryEntries: 0,
      payloadBytes: 0,
      payloadEntries: 0,
      extensionBytes: 0,
      extensionEntries: 0,
      decodedFeatureBytes: 0,
      decodedFeatureEntries: 0,
    }),
    clearCaches: () => undefined,
    close: () => undefined,
  };
}

function makeTrace(start: number): QueryTrace {
  return {
    dependencyRounds: 1,
    requestRanges: [],
    totalBytes: 0,
    uniqueBytes: 0,
    duplicateBytes: 0,
    bootstrapBytes: 0,
    directoryBytes: 0,
    payloadBytes: 0,
    cacheHits: { bootstrap: 0, directory: 0, payload: 0 },
    integrityMs: 0,
    decompressionMs: 0,
    decompressionTaskMs: 0,
    decodeMs: 0,
    mergeMs: 0,
    selectedChunks: 1,
    selectedNodes: 4,
    selectedTraversals: 2,
    canonicalHash: String(start),
  };
}

function delay(milliseconds: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

function fakeCanvasContext(): {
  value: CanvasRenderingContext2D;
  clearRect: ReturnType<typeof vi.fn>;
  fillRect: ReturnType<typeof vi.fn>;
  quadraticCurveTo: ReturnType<typeof vi.fn>;
  bezierCurveTo: ReturnType<typeof vi.fn>;
  fillText: ReturnType<typeof vi.fn>;
} {
  const clearRect = vi.fn();
  const fillRect = vi.fn();
  const quadraticCurveTo = vi.fn();
  const bezierCurveTo = vi.fn();
  const fillText = vi.fn();
  const value = {
    save: vi.fn(),
    restore: vi.fn(),
    clearRect,
    fillRect,
    beginPath: vi.fn(),
    moveTo: vi.fn(),
    lineTo: vi.fn(),
    quadraticCurveTo,
    bezierCurveTo,
    stroke: vi.fn(),
    fill: vi.fn(),
    rect: vi.fn(),
    roundRect: vi.fn(),
    closePath: vi.fn(),
    setLineDash: vi.fn(),
    measureText: vi.fn(() => ({ width: 60 })),
    fillText,
  } as unknown as CanvasRenderingContext2D;
  return {
    value,
    clearRect,
    fillRect,
    quadraticCurveTo,
    bezierCurveTo,
    fillText,
  };
}
