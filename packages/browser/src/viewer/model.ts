import type { RegionQuery, RegionTile } from "../reader/types.js";
import type {
  ViewerBudgets,
  ViewerCounts,
  ViewerLayout,
  ViewerLayoutEdge,
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
  readonly #edgeIndexes = new Map<string, number>();
  readonly #referenceOrder = new Map<
    bigint,
    {
      readonly handle: bigint;
      readonly tileStart: number;
      readonly index: number;
    }
  >();
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
    for (let index = 0; index < tile.referenceTraversal.length; index += 1) {
      const orientedNode = tile.referenceTraversal[index];
      if (orientedNode === undefined) continue;
      const id = nodeId(orientedNode);
      tileReferenceOrientations.set(id, isReverse(orientedNode));
      const existingOrder = this.#referenceOrder.get(id);
      if (
        existingOrder === undefined ||
        tile.coreStart < existingOrder.tileStart ||
        (tile.coreStart === existingOrder.tileStart &&
          index < existingOrder.index)
      ) {
        this.#referenceOrder.set(id, {
          handle: orientedNode,
          tileStart: tile.coreStart,
          index,
        });
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
      if (this.#edgeKeys.has(key)) {
        const edgeIndex = this.#edgeIndexes.get(key);
        const existing =
          edgeIndex === undefined ? undefined : this.#edges[edgeIndex];
        if (existing !== undefined) {
          const source = nodeSource(tile);
          if (!hasNodeSource(existing.sourceTiles, source)) {
            this.#edges[edgeIndex as number] = {
              ...existing,
              sourceTiles: [...existing.sourceTiles, source].sort(
                compareNodeSources,
              ),
            };
          }
        }
        continue;
      }
      this.#edgeKeys.add(key);
      this.#edgeIndexes.set(key, this.#edges.length);
      this.#edges.push({
        from,
        to,
        fromReverse: isReverse(fromHandle),
        toReverse: isReverse(toHandle),
        sourceTiles: [nodeSource(tile)],
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
        source: nodeSource(tile),
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
      referenceTraversal: [...this.#referenceOrder.values()]
        .sort(
          (left, right) =>
            left.tileStart - right.tileStart ||
            left.index - right.index ||
            compareBigints(nodeId(left.handle), nodeId(right.handle)),
        )
        .map(({ handle }) => handle),
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
        const source = nodeSource(tile);
        const sourceTiles = existing.sourceTiles.some(
          (item) =>
            item.archiveOffset === source.archiveOffset &&
            item.coreStart === source.coreStart &&
            item.coreEnd === source.coreEnd,
        )
          ? existing.sourceTiles
          : [...existing.sourceTiles, source].sort(compareNodeSources);
        if (referencePass && !existing.reference) {
          this.#nodes.set(id, { ...existing, reference: true, sourceTiles });
        } else if (sourceTiles !== existing.sourceTiles) {
          this.#nodes.set(id, { ...existing, sourceTiles });
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
        sourceTiles: [nodeSource(tile)],
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

  const components = alternateComponents(model, referenceCoordinates);
  const componentByNode = new Map<bigint, AlternateComponent>();
  const positiveLaneEnds: number[] = [];
  const negativeLaneEnds: number[] = [];
  for (const component of components) {
    const laneEnds =
      component.direction > 0 ? positiveLaneEnds : negativeLaneEnds;
    component.lane = allocateIntervalLane(
      laneEnds,
      component.anchorStart,
      component.anchorEnd,
    );
    for (const id of component.nodes) componentByNode.set(id, component);
  }
  const nodes: ViewerLayoutNode[] = [];
  const nodePositions = new Map<bigint, ViewerLayoutNode>();
  for (const node of [...model.nodes.values()].sort((leftNode, rightNode) =>
    leftNode.id < rightNode.id ? -1 : leftNode.id > rightNode.id ? 1 : 0,
  )) {
    const component = componentByNode.get(node.id);
    const componentIndex = component?.nodes.indexOf(node.id) ?? 0;
    const componentFraction =
      component === undefined
        ? 0.5
        : (componentIndex + 1) / (component.nodes.length + 1);
    const coordinate =
      referenceCoordinates.get(node.id) ??
      (component === undefined
        ? (node.tileStart + node.tileEnd) / 2
        : component.anchorStart +
          (component.anchorEnd - component.anchorStart) * componentFraction);
    const baseWidth =
      (Math.max(1, node.sequenceLength) / totalReferenceBases) *
      plotWidth *
      zoom;
    const nodeWidth = clamp(baseWidth, 10, 132);
    let y = referenceY;
    if (component !== undefined)
      y += component.direction * (44 + component.lane * 34);
    const x = worldX(coordinate) - nodeWidth / 2;
    const layoutNode: ViewerLayoutNode = {
      ...node,
      x,
      y,
      width: nodeWidth,
      height: 22,
      visible: x + nodeWidth >= left - 150 && x <= width + 150,
      lane: component?.lane ?? 0,
      branchKind: component?.kind ?? "reference",
      anchorStart: component?.anchorStart ?? coordinate,
      anchorEnd: component?.anchorEnd ?? coordinate,
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
    const isReference = referenceEdgeKeys.has(
      `${edge.from.toString()}:${edge.to.toString()}`,
    );
    const fromComponent = componentByNode.get(edge.from);
    const toComponent = componentByNode.get(edge.to);
    const classification: ViewerLayoutEdge["classification"] = isReference
      ? "reference"
      : fromComponent?.kind === "inversion" || toComponent?.kind === "inversion"
        ? "inversion"
        : referenceCoordinates.has(edge.from) &&
            referenceCoordinates.has(edge.to)
          ? "deletion"
          : "alternate";
    return [
      {
        ...edge,
        fromX: from.x + from.width / 2,
        fromY: from.y + from.height / 2,
        toX: to.x + to.width / 2,
        toY: to.y + to.height / 2,
        reference: isReference,
        classification,
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
    orientedNodes: traversal.orientedNodes,
    tileStart: traversal.tileStart,
    tileEnd: traversal.tileEnd,
    lane,
    source: traversal.source,
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

export function hitTestTraversal(
  layout: ViewerLayout,
  x: number,
  y: number,
): ViewerLayout["traversals"][number] | undefined {
  for (let index = layout.traversals.length - 1; index >= 0; index -= 1) {
    const traversal = layout.traversals[index];
    if (traversal === undefined) continue;
    for (
      let pointIndex = 1;
      pointIndex < traversal.points.length;
      pointIndex += 1
    ) {
      const from = traversal.points[pointIndex - 1];
      const to = traversal.points[pointIndex];
      if (
        from !== undefined &&
        to !== undefined &&
        pointSegmentDistance(x, y, from.x, from.y, to.x, to.y) <= 6
      ) {
        return traversal;
      }
    }
  }
  return undefined;
}

export function hitTestEdge(
  layout: ViewerLayout,
  x: number,
  y: number,
): ViewerLayout["edges"][number] | undefined {
  for (let index = layout.edges.length - 1; index >= 0; index -= 1) {
    const edge = layout.edges[index];
    if (
      edge !== undefined &&
      pointSegmentDistance(x, y, edge.fromX, edge.fromY, edge.toX, edge.toY) <=
        6
    ) {
      return edge;
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

interface AlternateComponent {
  readonly nodes: bigint[];
  readonly anchorStart: number;
  readonly anchorEnd: number;
  readonly kind: "alternate" | "insertion" | "inversion" | "unanchored";
  readonly direction: -1 | 1;
  lane: number;
}

function alternateComponents(
  model: ViewerModel,
  referenceCoordinates: ReadonlyMap<bigint, number>,
): AlternateComponent[] {
  const adjacency = new Map<bigint, bigint[]>();
  for (const edge of model.edges) {
    appendNeighbor(adjacency, edge.from, edge.to);
    appendNeighbor(adjacency, edge.to, edge.from);
  }
  const alternateIds = [...model.nodes.values()]
    .filter((node) => !node.reference)
    .map((node) => node.id)
    .sort(compareBigints);
  const visited = new Set<bigint>();
  const components: AlternateComponent[] = [];
  for (const seed of alternateIds) {
    if (visited.has(seed)) continue;
    const stack = [seed];
    const nodes: bigint[] = [];
    const anchors: number[] = [];
    while (stack.length > 0) {
      const id = stack.pop();
      if (id === undefined || visited.has(id)) continue;
      visited.add(id);
      nodes.push(id);
      for (const neighbor of adjacency.get(id) ?? []) {
        const anchor = referenceCoordinates.get(neighbor);
        if (anchor !== undefined) anchors.push(anchor);
        else if (
          !visited.has(neighbor) &&
          model.nodes.get(neighbor)?.reference === false
        ) {
          stack.push(neighbor);
        }
      }
    }
    nodes.sort(compareBigints);
    anchors.sort((leftAnchor, rightAnchor) => leftAnchor - rightAnchor);
    const sourceNodes = nodes.flatMap((id) => {
      const node = model.nodes.get(id);
      return node === undefined ? [] : [node];
    });
    const fallbackStart = Math.min(
      ...sourceNodes.map((node) => node.tileStart),
    );
    const fallbackEnd = Math.max(...sourceNodes.map((node) => node.tileEnd));
    const anchorStart = anchors.at(0) ?? fallbackStart;
    const anchorEnd = anchors.at(-1) ?? fallbackEnd;
    const hasReverse = sourceNodes.some((node) => node.reverse);
    const kind =
      anchors.length === 0
        ? "unanchored"
        : hasReverse
          ? "inversion"
          : anchorEnd - anchorStart <= 1
            ? "insertion"
            : "alternate";
    const seedValue = nodes[0] ?? 0n;
    components.push({
      nodes,
      anchorStart,
      anchorEnd: Math.max(anchorStart, anchorEnd),
      kind,
      direction: seedValue % 2n === 0n ? -1 : 1,
      lane: 0,
    });
  }
  return components.sort(
    (leftComponent, rightComponent) =>
      leftComponent.anchorStart - rightComponent.anchorStart ||
      leftComponent.anchorEnd - rightComponent.anchorEnd ||
      compareBigints(
        leftComponent.nodes[0] ?? 0n,
        rightComponent.nodes[0] ?? 0n,
      ),
  );
}

function allocateIntervalLane(
  laneEnds: number[],
  start: number,
  end: number,
): number {
  const paddedStart = start - 1;
  for (let lane = 0; lane < laneEnds.length; lane += 1) {
    if ((laneEnds[lane] ?? Number.NEGATIVE_INFINITY) <= paddedStart) {
      laneEnds[lane] = end;
      return lane;
    }
  }
  laneEnds.push(end);
  return laneEnds.length - 1;
}

function appendNeighbor(
  adjacency: Map<bigint, bigint[]>,
  from: bigint,
  to: bigint,
): void {
  const values = adjacency.get(from);
  if (values === undefined) adjacency.set(from, [to]);
  else values.push(to);
}

function compareBigints(left: bigint, right: bigint): number {
  return left < right ? -1 : left > right ? 1 : 0;
}

function nodeSource(tile: RegionTile) {
  return {
    coreStart: tile.coreStart,
    coreEnd: tile.coreEnd,
    archiveOffset: tile.provenance.archiveOffset,
    compressedBytes: tile.provenance.compressedBytes,
    uncompressedBytes: tile.provenance.uncompressedBytes,
  };
}

function compareNodeSources(
  left: ViewerModelNode["sourceTiles"][number],
  right: ViewerModelNode["sourceTiles"][number],
): number {
  return (
    left.coreStart - right.coreStart ||
    left.coreEnd - right.coreEnd ||
    compareBigints(left.archiveOffset, right.archiveOffset)
  );
}

function hasNodeSource(
  sources: readonly ViewerModelNode["sourceTiles"][number][],
  candidate: ViewerModelNode["sourceTiles"][number],
): boolean {
  return sources.some(
    (source) =>
      source.archiveOffset === candidate.archiveOffset &&
      source.coreStart === candidate.coreStart &&
      source.coreEnd === candidate.coreEnd,
  );
}

function pointSegmentDistance(
  x: number,
  y: number,
  x1: number,
  y1: number,
  x2: number,
  y2: number,
): number {
  const dx = x2 - x1;
  const dy = y2 - y1;
  const lengthSquared = dx * dx + dy * dy;
  const fraction =
    lengthSquared === 0
      ? 0
      : clamp(((x - x1) * dx + (y - y1) * dy) / lengthSquared, 0, 1);
  return Math.hypot(x - (x1 + dx * fraction), y - (y1 + dy * fraction));
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
