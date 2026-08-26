import type { RegionQuery, RegionTile } from "../reader/types.js";
import type {
  ViewerBudgets,
  ViewerCounts,
  ViewerLayout,
  ViewerLayoutNode,
  ViewerLayoutOptions,
  ViewerModel,
  ViewerModelEdge,
  ViewerModelNode,
  ViewerModelTraversal,
  ViewerTileBoundary,
} from "./types.js";

const DEFAULT_BUDGETS: ViewerBudgets = {
  maxRenderedNodes: 2_000,
  maxRenderedEdges: 4_000,
  maxHaplotypeLanes: 24,
};

const EMPTY_COUNTS: ViewerCounts = {
  tiles: 0,
  decodedNodes: 0,
  decodedEdges: 0,
  decodedTraversals: 0,
  renderedNodes: 0,
  renderedEdges: 0,
  renderedHaplotypeLanes: 0,
  omittedNodes: 0,
  omittedEdges: 0,
  omittedTraversals: 0,
};

const decoder = new TextDecoder();

export function viewerBudgets(
  options: Partial<ViewerBudgets> = {},
): ViewerBudgets {
  return {
    maxRenderedNodes: positiveInteger(
      options.maxRenderedNodes,
      DEFAULT_BUDGETS.maxRenderedNodes,
    ),
    maxRenderedEdges: positiveInteger(
      options.maxRenderedEdges,
      DEFAULT_BUDGETS.maxRenderedEdges,
    ),
    maxHaplotypeLanes: positiveInteger(
      options.maxHaplotypeLanes,
      DEFAULT_BUDGETS.maxHaplotypeLanes,
    ),
  };
}

/**
 * Incremental, bounded view-model construction. Anonymous traversals are kept
 * with their source tile and are never stitched across tile boundaries.
 */
export class ViewerModelBuilder {
  readonly #query: Readonly<RegionQuery>;
  readonly #budgets: ViewerBudgets;
  readonly #nodes = new Map<bigint, ViewerModelNode>();
  readonly #edges: ViewerModelEdge[] = [];
  readonly #edgeKeys = new Set<string>();
  readonly #referenceTraversal: bigint[] = [];
  readonly #referenceIds = new Set<bigint>();
  readonly #traversals: ViewerModelTraversal[] = [];
  readonly #tileBoundaries: ViewerTileBoundary[] = [];
  #semantics: RegionTile["semantics"] | undefined;
  #tiles = 0;
  #decodedNodes = 0;
  #decodedEdges = 0;
  #decodedTraversals = 0;

  constructor(query: RegionQuery, budgets: ViewerBudgets = DEFAULT_BUDGETS) {
    this.#query = copyQuery(query);
    this.#budgets = viewerBudgets(budgets);
  }

  addTile(tile: RegionTile): void {
    this.#tiles += 1;
    this.#decodedNodes += tile.nodes.ids.length;
    this.#decodedEdges += tile.topology.from.length;
    this.#decodedTraversals += tile.haplotypes.weights.length;
    this.#semantics = tile.semantics;

    if (this.#tileBoundaries.length < 4_096) {
      this.#tileBoundaries.push({
        coreStart: tile.coreStart,
        coreEnd: tile.coreEnd,
        archiveOffset: tile.provenance.archiveOffset,
      });
    }

    const tileReferenceOrientations = new Map<bigint, boolean>();
    for (const orientedNode of tile.referenceTraversal) {
      const id = nodeId(orientedNode);
      tileReferenceOrientations.set(id, isReverse(orientedNode));
      if (!this.#referenceIds.has(id)) {
        this.#referenceIds.add(id);
        this.#referenceTraversal.push(orientedNode);
      }
    }

    // Preserve reference structure first when a tile exceeds the node budget.
    this.#retainNodes(tile, tileReferenceOrientations, true);
    this.#retainNodes(tile, tileReferenceOrientations, false);

    const edgeCount = Math.min(
      tile.topology.from.length,
      tile.topology.to.length,
    );
    for (let index = 0; index < edgeCount; index += 1) {
      if (this.#edges.length >= this.#budgets.maxRenderedEdges) break;
      const fromHandle = tile.topology.from[index];
      const toHandle = tile.topology.to[index];
      if (fromHandle === undefined || toHandle === undefined) continue;
      const from = nodeId(fromHandle);
      const to = nodeId(toHandle);
      if (!this.#nodes.has(from) || !this.#nodes.has(to)) continue;
      const key = `${fromHandle.toString()}:${toHandle.toString()}`;
      if (this.#edgeKeys.has(key)) continue;
      this.#edgeKeys.add(key);
      this.#edges.push({
        from,
        to,
        fromReverse: isReverse(fromHandle),
        toReverse: isReverse(toHandle),
      });
    }

    const offsets = tile.haplotypes.traversalOffsets;
    for (let index = 0; index < tile.haplotypes.weights.length; index += 1) {
      const start = offsets[index];
      const end = offsets[index + 1];
      const weight = tile.haplotypes.weights[index];
      if (start === undefined || end === undefined || weight === undefined)
        continue;
      const worst = this.#traversals.at(-1);
      if (
        this.#traversals.length >= this.#budgets.maxHaplotypeLanes &&
        worst !== undefined &&
        weight <= worst.weight
      ) {
        continue;
      }
      const orientedNodes = Array.from(
        tile.haplotypes.orientedNodes.subarray(start, end),
      );
      this.#traversals.push({
        tileStart: tile.coreStart,
        tileEnd: tile.coreEnd,
        orientedNodes,
        weight,
      });
      this.#traversals.sort(compareTraversals);
      this.#traversals.length = Math.min(
        this.#traversals.length,
        this.#budgets.maxHaplotypeLanes,
      );
    }
  }

  snapshot(): ViewerModel {
    const renderedNodes = this.#nodes.size;
    const renderedEdges = this.#edges.length;
    const renderedHaplotypeLanes = this.#traversals.length;
    const counts: ViewerCounts = {
      tiles: this.#tiles,
      decodedNodes: this.#decodedNodes,
      decodedEdges: this.#decodedEdges,
      decodedTraversals: this.#decodedTraversals,
      renderedNodes,
      renderedEdges,
      renderedHaplotypeLanes,
      omittedNodes: Math.max(0, this.#decodedNodes - renderedNodes),
      omittedEdges: Math.max(0, this.#decodedEdges - renderedEdges),
      omittedTraversals: Math.max(
        0,
        this.#decodedTraversals - renderedHaplotypeLanes,
      ),
    };
    return {
      query: this.#query,
      budgets: this.#budgets,
      nodes: this.#nodes,
      edges: this.#edges,
      referenceTraversal: this.#referenceTraversal,
      traversals: this.#traversals,
      tileBoundaries: this.#tileBoundaries,
      counts,
      semantics: this.#semantics,
    };
  }

  #retainNodes(
    tile: RegionTile,
    referenceOrientations: ReadonlyMap<bigint, boolean>,
    referencePass: boolean,
  ): void {
    for (let index = 0; index < tile.nodes.ids.length; index += 1) {
      const id = tile.nodes.ids[index];
      if (id === undefined || referenceOrientations.has(id) !== referencePass) {
        continue;
      }
      const existing = this.#nodes.get(id);
      if (existing !== undefined) {
        if (referencePass && !existing.reference) {
          this.#nodes.set(id, { ...existing, reference: true });
        }
        continue;
      }
      if (this.#nodes.size >= this.#budgets.maxRenderedNodes) return;
      const start = tile.nodes.sequenceOffsets[index];
      const end = tile.nodes.sequenceOffsets[index + 1];
      if (start === undefined || end === undefined || end < start) continue;
      const sequenceBytes = tile.nodes.sequenceBytes.subarray(start, end);
      this.#nodes.set(id, {
        id,
        sequence: decoder.decode(sequenceBytes),
        sequenceLength: sequenceBytes.length,
        reference: referencePass,
        reverse: referenceOrientations.get(id) ?? false,
        tileStart: tile.coreStart,
        tileEnd: tile.coreEnd,
      });
    }
  }
}

export function emptyViewerCounts(): ViewerCounts {
  return { ...EMPTY_COUNTS };
}

export function layoutViewerModel(
  model: ViewerModel,
  options: ViewerLayoutOptions,
): ViewerLayout {
  const width = Math.max(240, options.width);
  const height = Math.max(240, options.height);
  const zoom = clamp(options.zoom ?? 1, 0.5, 24);
  const panX = options.panX ?? 0;
  const left = 54;
  const right = 24;
  const plotWidth = Math.max(1, width - left - right);
  const referenceY = Math.round(height * 0.42);
  const interval = Math.max(1, model.query.end - model.query.start);
  const worldX = (coordinate: number): number =>
    left +
    ((coordinate - model.query.start) / interval) * plotWidth * zoom +
    panX;

  const referenceHandles = model.referenceTraversal.filter((handle) =>
    model.nodes.has(nodeId(handle)),
  );
  const totalReferenceBases = Math.max(
    1,
    referenceHandles.reduce(
      (sum, handle) =>
        sum + (model.nodes.get(nodeId(handle))?.sequenceLength ?? 1),
      0,
    ),
  );
  const referenceCoordinates = new Map<bigint, number>();
  let cumulativeBases = 0;
  for (const handle of referenceHandles) {
    const id = nodeId(handle);
    const node = model.nodes.get(id);
    if (node === undefined) continue;
    const midpoint = cumulativeBases + node.sequenceLength / 2;
    referenceCoordinates.set(
      id,
      model.query.start + (midpoint / totalReferenceBases) * interval,
    );
    cumulativeBases += node.sequenceLength;
  }

  const adjacentReferenceCoordinates = new Map<bigint, number[]>();
  for (const edge of model.edges) {
    const fromCoordinate = referenceCoordinates.get(edge.from);
    const toCoordinate = referenceCoordinates.get(edge.to);
    if (fromCoordinate !== undefined && toCoordinate === undefined) {
      appendCoordinate(adjacentReferenceCoordinates, edge.to, fromCoordinate);
    }
    if (toCoordinate !== undefined && fromCoordinate === undefined) {
      appendCoordinate(adjacentReferenceCoordinates, edge.from, toCoordinate);
    }
  }

  const nodes: ViewerLayoutNode[] = [];
  const nodePositions = new Map<bigint, ViewerLayoutNode>();
  let alternativeIndex = 0;
  for (const node of model.nodes.values()) {
    const adjacent = adjacentReferenceCoordinates.get(node.id);
    const coordinate =
      referenceCoordinates.get(node.id) ??
      (adjacent === undefined
        ? (node.tileStart + node.tileEnd) / 2
        : adjacent.reduce((sum, value) => sum + value, 0) / adjacent.length);
    const baseWidth =
      (Math.max(1, node.sequenceLength) / totalReferenceBases) *
      plotWidth *
      zoom;
    const nodeWidth = clamp(baseWidth, 10, 132);
    let y = referenceY;
    if (!node.reference) {
      const branch = alternativeIndex % 6;
      const direction = branch % 2 === 0 ? -1 : 1;
      y += direction * (42 + Math.floor(branch / 2) * 34);
      alternativeIndex += 1;
    }
    const x = worldX(coordinate) - nodeWidth / 2;
    const layoutNode: ViewerLayoutNode = {
      ...node,
      x,
      y,
      width: nodeWidth,
      height: 22,
      visible: x + nodeWidth >= left - 150 && x <= width + 150,
    };
    nodes.push(layoutNode);
    nodePositions.set(node.id, layoutNode);
  }

  const referenceEdgeKeys = new Set<string>();
  for (let index = 0; index + 1 < referenceHandles.length; index += 1) {
    const from = referenceHandles[index];
    const to = referenceHandles[index + 1];
    if (from !== undefined && to !== undefined) {
      referenceEdgeKeys.add(
        `${nodeId(from).toString()}:${nodeId(to).toString()}`,
      );
    }
  }
  const edges = model.edges.flatMap((edge) => {
    const from = nodePositions.get(edge.from);
    const to = nodePositions.get(edge.to);
    if (from === undefined || to === undefined) return [];
    return [
      {
        ...edge,
        fromX: from.x + from.width / 2,
        fromY: from.y + from.height / 2,
        toX: to.x + to.width / 2,
        toY: to.y + to.height / 2,
        reference: referenceEdgeKeys.has(
          `${edge.from.toString()}:${edge.to.toString()}`,
        ),
      },
    ];
  });

  const traversalTop = Math.round(height * 0.68);
  const traversals = model.traversals.map((traversal, lane) => ({
    points: traversal.orientedNodes.flatMap((handle) => {
      const node = nodePositions.get(nodeId(handle));
      return node === undefined
        ? []
        : [{ x: node.x + node.width / 2, y: traversalTop + lane * 10 }];
    }),
    weight: traversal.weight,
    tileStart: traversal.tileStart,
    tileEnd: traversal.tileEnd,
    lane,
  }));

  return {
    width,
    height,
    query: model.query,
    nodes,
    edges,
    traversals,
    tileBoundaries: model.tileBoundaries.map((tile) => ({
      ...tile,
      x: worldX(tile.coreStart),
    })),
    counts: model.counts,
    semantics: model.semantics,
    zoom,
    panX,
  };
}

export function hitTestNode(
  layout: ViewerLayout,
  x: number,
  y: number,
): ViewerLayoutNode | undefined {
  for (let index = layout.nodes.length - 1; index >= 0; index -= 1) {
    const node = layout.nodes[index];
    if (
      node?.visible &&
      x >= node.x &&
      x <= node.x + node.width &&
      y >= node.y &&
      y <= node.y + node.height
    ) {
      return node;
    }
  }
  return undefined;
}

export function viewerSummary(counts: ViewerCounts): string | undefined {
  const parts: string[] = [];
  if (counts.omittedNodes > 0)
    parts.push(`${counts.omittedNodes} node occurrences`);
  if (counts.omittedEdges > 0) parts.push(`${counts.omittedEdges} edges`);
  if (counts.omittedTraversals > 0) {
    parts.push(`${counts.omittedTraversals} weighted local traversals`);
  }
  return parts.length === 0
    ? undefined
    : `Rendering budget reached; summarized ${parts.join(", ")}.`;
}

function nodeId(orientedNode: bigint): bigint {
  return orientedNode >> 1n;
}

function isReverse(orientedNode: bigint): boolean {
  return (orientedNode & 1n) === 1n;
}

function copyQuery(query: RegionQuery): Readonly<RegionQuery> {
  const copied: RegionQuery = {
    sample: query.sample,
    contig: query.contig,
    start: query.start,
    end: query.end,
  };
  if (query.context !== undefined) copied.context = query.context;
  return copied;
}

function compareTraversals(
  left: ViewerModelTraversal,
  right: ViewerModelTraversal,
): number {
  if (left.weight !== right.weight) return left.weight > right.weight ? -1 : 1;
  if (left.tileStart !== right.tileStart)
    return left.tileStart - right.tileStart;
  return left.orientedNodes.length - right.orientedNodes.length;
}

function appendCoordinate(
  coordinates: Map<bigint, number[]>,
  id: bigint,
  coordinate: number,
): void {
  const values = coordinates.get(id);
  if (values === undefined) coordinates.set(id, [coordinate]);
  else values.push(coordinate);
}

function positiveInteger(value: number | undefined, fallback: number): number {
  if (value === undefined) return fallback;
  if (!Number.isSafeInteger(value) || value <= 0) {
    throw new RangeError(
      `viewer budgets must be positive safe integers; got ${value}`,
    );
  }
  return value;
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(minimum, value));
}
