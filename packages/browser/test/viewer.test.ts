import { describe, expect, it, vi } from "vitest";
import type {
  PangenomeArchive,
  QueryTrace,
  RegionQuery,
  RegionTile,
} from "../src/reader/types.js";
import {
  hitTestNode,
  layoutViewerModel,
  ViewerModelBuilder,
  viewerBudgets,
  viewerSummary,
} from "../src/viewer/model.js";
import { ProgressiveTileQuery } from "../src/viewer/query-controller.js";
import { renderViewerCanvas } from "../src/viewer/renderer.js";

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

function makeTile(archiveOffset: bigint): RegionTile {
  const sequenceBytes = new TextEncoder().encode("ACGTT");
  const ids = BigUint64Array.from([1n, 2n, 3n, 4n]);
  const sequenceOffsets = Uint32Array.from([0, 1, 2, 4, 5]);
  const edgeFrom = BigUint64Array.from([2n, 6n, 6n]);
  const edgeTo = BigUint64Array.from([4n, 4n, 8n]);
  const referenceTraversal = BigUint64Array.from([2n, 4n]);
  const traversalOffsets = Uint32Array.from([0, 2, 5]);
  const traversalNodes = BigUint64Array.from([2n, 4n, 2n, 6n, 4n]);
  const traversalWeights = BigUint64Array.from([10n, 3n]);
  return {
    reference: { sample: "GRCh38", contig: "chr1", start: 100, end: 200 },
    coreStart: 100,
    coreEnd: 200,
    start: 100,
    end: 200,
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
    edges: BigUint64Array.from([2n, 4n, 6n, 4n, 6n, 8n]),
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
    decompressionMs: 0,
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
  fillText: ReturnType<typeof vi.fn>;
} {
  const clearRect = vi.fn();
  const fillRect = vi.fn();
  const quadraticCurveTo = vi.fn();
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
    stroke: vi.fn(),
    fill: vi.fn(),
    rect: vi.fn(),
    closePath: vi.fn(),
    setLineDash: vi.fn(),
    fillText,
  } as unknown as CanvasRenderingContext2D;
  return { value, clearRect, fillRect, quadraticCurveTo, fillText };
}
