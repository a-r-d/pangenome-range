import type {
  LocalPattern,
  TubeMapEdge,
  TubeMapModel,
  TubeMapNode,
} from "./tube-map-model.js";
import { patternThickness } from "./tube-map-model.js";

export interface TubeMapLayoutOptions {
  readonly width: number;
  readonly height: number;
  readonly zoom?: number;
  readonly panX?: number;
  readonly showBases?: "automatic" | "on" | "off";
}

export interface TubeMapLayoutNode extends TubeMapNode {
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
  readonly lane: number;
  readonly label: string;
  readonly ariaLabel: string;
  readonly showLabel: boolean;
  readonly showSequence: boolean;
}

export interface TubeMapLayoutEdge extends TubeMapEdge {
  readonly path: string;
  readonly lane: number;
}

export interface TubeMapLayoutPattern extends LocalPattern {
  readonly path: string;
  readonly thickness: number;
  readonly lane: number;
  readonly labelX: number;
  readonly labelY: number;
}

export interface TubeMapLayoutBoundary {
  readonly tileKey: string;
  readonly x: number;
  readonly start: number;
  readonly end: number;
}

export interface TubeMapLayout {
  readonly model: TubeMapModel;
  readonly width: number;
  readonly height: number;
  readonly contentWidth: number;
  readonly referenceY: number;
  readonly nodes: readonly TubeMapLayoutNode[];
  readonly edges: readonly TubeMapLayoutEdge[];
  readonly patterns: readonly TubeMapLayoutPattern[];
  readonly tileBoundaries: readonly TubeMapLayoutBoundary[];
  readonly elementCount: number;
}

const NODE_HEIGHT = 34;
const REFERENCE_GAP = 24;
const SIDE_PADDING = 72;
const PATTERN_LANE_GAP = 12;

/** Deterministic reference-anchored SVG layout with no force simulation. */
export function layoutTubeMap(
  model: TubeMapModel,
  options: TubeMapLayoutOptions,
): TubeMapLayout {
  const width = positiveDimension(options.width, "width");
  const height = positiveDimension(options.height, "height");
  const zoom = clamp(options.zoom ?? 1, 0.2, 5);
  const panX = Number.isFinite(options.panX) ? (options.panX ?? 0) : 0;
  const referenceY = Math.max(120, Math.round(height * 0.5));
  const nodesByKey = new Map(model.nodes.map((node) => [node.key, node]));
  const referenceSet = new Set(model.reference);
  const referencePositions = new Map<string, number>();
  const referenceWidth = model.reference.reduce((total, key, index) => {
    const node = nodesByKey.get(key);
    if (node === undefined) return total;
    return (
      total +
      nodeWidth(node) * zoom +
      (index === model.reference.length - 1 ? 0 : REFERENCE_GAP * zoom)
    );
  }, 0);
  let cursor = Math.max(SIDE_PADDING, (width - referenceWidth) / 2) + panX;
  for (const key of model.reference) {
    const node = nodesByKey.get(key);
    if (node === undefined) continue;
    referencePositions.set(key, cursor);
    cursor += nodeWidth(node) * zoom + REFERENCE_GAP * zoom;
  }
  const contentWidth = Math.max(width, cursor + SIDE_PADDING - panX);
  const components = alternateComponents(model, referenceSet);
  const alternatePlacement = new Map<string, { x: number; lane: number }>();
  const occupied = new Map<number, Array<readonly [number, number]>>();
  for (const component of components) {
    const attachments = referenceAttachments(
      component,
      model,
      referencePositions,
    );
    const left = attachments[0] ?? SIDE_PADDING;
    const right = attachments[1] ?? left + 180 * zoom;
    const componentWidth = component.reduce(
      (sum, key) =>
        sum +
        nodeWidth(nodesByKey.get(key) ?? { sequenceLength: 1 }) * zoom +
        14 * zoom,
      0,
    );
    const start = Math.max(
      SIDE_PADDING + panX,
      (left + right - componentWidth) / 2,
    );
    const end = start + componentWidth;
    const lane = firstAvailableLane(start, end, occupied, component[0] ?? "");
    const intervals = occupied.get(lane) ?? [];
    intervals.push([start, end]);
    occupied.set(lane, intervals);
    let x = start;
    for (const key of component) {
      alternatePlacement.set(key, { x, lane });
      x +=
        nodeWidth(nodesByKey.get(key) ?? { sequenceLength: 1 }) * zoom +
        14 * zoom;
    }
  }
  const maxLaneDepth = Math.max(
    1,
    ...[...alternatePlacement.values()].map(({ lane }) => Math.abs(lane)),
  );
  const laneGap = Math.min(
    68,
    Math.max(26, (height - 110) / (2 * (maxLaneDepth + 1))),
  );

  const nodes: TubeMapLayoutNode[] = model.nodes.flatMap((node) => {
    const referenceX = referencePositions.get(node.key);
    const alternate = alternatePlacement.get(node.key);
    const x = referenceX ?? alternate?.x;
    if (x === undefined) return [];
    const lane = referenceX === undefined ? (alternate?.lane ?? 1) : 0;
    const y = referenceY + lane * laneGap;
    const displayWidth = nodeWidth(node) * zoom;
    const ariaLabel = nodeLabel(node);
    const label = visibleNodeLabel(node, displayWidth);
    return [
      {
        ...node,
        x,
        y,
        width: displayWidth,
        height: NODE_HEIGHT,
        lane,
        label: label ?? ariaLabel,
        ariaLabel,
        showLabel: label !== undefined,
        showSequence: shouldShowSequence(node, displayWidth, options.showBases),
      },
    ];
  });
  const positioned = new Map(nodes.map((node) => [node.key, node]));
  const edges = model.edges.flatMap((edge) => {
    const from = positioned.get(edge.from);
    const to = positioned.get(edge.to);
    if (from === undefined || to === undefined) return [];
    const lane = edge.reference
      ? 0
      : from.lane === 0 && to.lane === 0
        ? stableSide(edge.key) * 1
        : Math.abs(from.lane) >= Math.abs(to.lane)
          ? from.lane
          : to.lane;
    return [{ ...edge, lane, path: edgePath(from, to, lane, referenceY) }];
  });
  const patterns = model.patterns.flatMap((pattern, index) => {
    const pathNodes = pattern.nodeKeys.flatMap((key) => {
      const node = positioned.get(key);
      return node === undefined ? [] : [node];
    });
    if (pathNodes.length === 0) return [];
    const lane =
      index % 2 === 0 ? -2 - Math.floor(index / 2) : 2 + Math.floor(index / 2);
    const offset = lane * PATTERN_LANE_GAP;
    const points = pathNodes.map(
      (node) => [node.x + node.width / 2, node.y + offset] as const,
    );
    const first = points[0];
    if (first === undefined) return [];
    return [
      {
        ...pattern,
        lane,
        path: smoothPath(points),
        thickness: patternThickness(pattern.weight),
        labelX: first[0],
        labelY: first[1] + Math.sign(lane) * 10,
      },
    ];
  });
  const referenceNodes = nodes.filter((node) => node.reference);
  const referenceLeft = referenceNodes[0]?.x ?? SIDE_PADDING + panX;
  const finalReference = referenceNodes.at(-1);
  const referenceRight =
    finalReference === undefined
      ? width - SIDE_PADDING + panX
      : finalReference.x + finalReference.width;
  const tileBoundaries = model.tileBoundaries.map((boundary) => {
    const sourceNode = referenceNodes.find(
      (node) => node.sourceTile.key === boundary.tileKey,
    );
    return {
      ...boundary,
      x:
        sourceNode?.x ??
        coordinateX(boundary.start, model, referenceLeft, referenceRight),
    };
  });
  return {
    model,
    width,
    height,
    contentWidth,
    referenceY,
    nodes,
    edges,
    patterns,
    tileBoundaries,
    elementCount:
      nodes.length + edges.length + patterns.length + tileBoundaries.length,
  };
}

export function nodeWidth(node: Pick<TubeMapNode, "sequenceLength">): number {
  return clamp(18 + Math.log2(node.sequenceLength + 1) * 8, 28, 140);
}

function alternateComponents(
  model: TubeMapModel,
  reference: ReadonlySet<string>,
): string[][] {
  const adjacency = new Map<string, Set<string>>();
  for (const node of model.nodes) {
    if (!reference.has(node.key)) adjacency.set(node.key, new Set());
  }
  for (const edge of model.edges) {
    if (reference.has(edge.from) || reference.has(edge.to)) continue;
    adjacency.get(edge.from)?.add(edge.to);
    adjacency.get(edge.to)?.add(edge.from);
  }
  const seen = new Set<string>();
  const result: string[][] = [];
  for (const seed of [...adjacency.keys()].sort()) {
    if (seen.has(seed)) continue;
    const queue = [seed];
    const component: string[] = [];
    seen.add(seed);
    while (queue.length > 0) {
      const key = queue.shift();
      if (key === undefined) continue;
      component.push(key);
      for (const neighbor of [...(adjacency.get(key) ?? [])].sort()) {
        if (seen.has(neighbor)) continue;
        seen.add(neighbor);
        queue.push(neighbor);
      }
    }
    result.push(component.sort());
  }
  return result;
}

function referenceAttachments(
  component: readonly string[],
  model: TubeMapModel,
  referencePositions: ReadonlyMap<string, number>,
): readonly [number, number] | readonly [] {
  const keys = new Set(component);
  const positions = model.edges.flatMap((edge) => {
    if (keys.has(edge.from)) {
      const x = referencePositions.get(edge.to);
      return x === undefined ? [] : [x];
    }
    if (keys.has(edge.to)) {
      const x = referencePositions.get(edge.from);
      return x === undefined ? [] : [x];
    }
    return [];
  });
  if (positions.length === 0) return [];
  return [Math.min(...positions), Math.max(...positions)];
}

function firstAvailableLane(
  start: number,
  end: number,
  occupied: ReadonlyMap<number, readonly (readonly [number, number])[]>,
  stableKey: string,
): number {
  const preferredSide = stableSide(stableKey);
  for (let depth = 1; depth <= 8; depth += 1) {
    for (const lane of [preferredSide * depth, -preferredSide * depth]) {
      const collision = (occupied.get(lane) ?? []).some(
        ([left, right]) => start < right + 16 && end > left - 16,
      );
      if (!collision) return lane;
    }
  }
  return preferredSide * 9;
}

function stableSide(key: string): -1 | 1 {
  let value = 0;
  for (const character of key)
    value = (value * 31 + character.charCodeAt(0)) >>> 0;
  return value % 2 === 0 ? -1 : 1;
}

function edgePath(
  from: TubeMapLayoutNode,
  to: TubeMapLayoutNode,
  lane: number,
  referenceY: number,
): string {
  const startX = from.x + from.width;
  const endX = to.x;
  const startY = from.y;
  const endY = to.y;
  if (from.lane === 0 && to.lane === 0 && lane !== 0) {
    const archY = referenceY + lane * 52;
    return `M ${round(startX)} ${round(startY)} C ${round(startX + 28)} ${round(archY)}, ${round(endX - 28)} ${round(archY)}, ${round(endX)} ${round(endY)}`;
  }
  const midpoint = (startX + endX) / 2;
  return `M ${round(startX)} ${round(startY)} C ${round(midpoint)} ${round(startY)}, ${round(midpoint)} ${round(endY)}, ${round(endX)} ${round(endY)}`;
}

function smoothPath(points: readonly (readonly [number, number])[]): string {
  const first = points[0];
  if (first === undefined) return "";
  let path = `M ${round(first[0])} ${round(first[1])}`;
  for (let index = 1; index < points.length; index += 1) {
    const previous = points[index - 1];
    const point = points[index];
    if (previous === undefined || point === undefined) continue;
    const midpoint = (previous[0] + point[0]) / 2;
    path += ` C ${round(midpoint)} ${round(previous[1])}, ${round(midpoint)} ${round(point[1])}, ${round(point[0])} ${round(point[1])}`;
  }
  return path;
}

function nodeLabel(node: TubeMapNode): string {
  const members = node.collapsedMembers?.length;
  return members === undefined
    ? node.id.toString()
    : `${members} nodes · ${formatBases(node.collapsedBaseLength ?? node.sequenceLength)}`;
}

function visibleNodeLabel(
  node: TubeMapNode,
  displayWidth: number,
): string | undefined {
  const full = nodeLabel(node);
  if (displayWidth >= estimatedLabelWidth(full)) return full;
  const members = node.collapsedMembers?.length;
  if (members === undefined) return undefined;
  const compact = `${members} nodes`;
  return displayWidth >= estimatedLabelWidth(compact) ? compact : undefined;
}

function estimatedLabelWidth(label: string): number {
  return label.length * 6.1 + 14;
}

function shouldShowSequence(
  node: TubeMapNode,
  width: number,
  mode: TubeMapLayoutOptions["showBases"],
): boolean {
  if (mode === "off" || node.collapsedMembers !== undefined) return false;
  if (mode === "on") return node.sequenceLength <= 48;
  return width >= 64 && node.sequenceLength <= 16;
}

function coordinateX(
  coordinate: number,
  model: TubeMapModel,
  referenceLeft: number,
  referenceRight: number,
): number {
  const span = Math.max(1, model.query.end - model.query.start);
  return (
    referenceLeft +
    ((coordinate - model.query.start) / span) * (referenceRight - referenceLeft)
  );
}

function formatBases(value: number): string {
  if (value >= 1_000)
    return `${(value / 1_000).toFixed(value >= 10_000 ? 0 : 1)} kb`;
  return `${value} bp`;
}

function positiveDimension(value: number, name: string): number {
  if (!Number.isFinite(value) || value <= 0) {
    throw new RangeError(`tube-map ${name} must be positive; got ${value}`);
  }
  return value;
}

function round(value: number): number {
  return Math.round(value * 10) / 10;
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value));
}
