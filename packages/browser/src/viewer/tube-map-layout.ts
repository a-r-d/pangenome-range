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
  readonly verticalScale?: number;
  readonly showBases?: "automatic" | "on" | "off";
}

export interface TubeMapVerticalFitOptions
  extends Omit<TubeMapLayoutOptions, "verticalScale"> {
  readonly minimumScale?: number;
  readonly maximumScale?: number;
  readonly verticalPadding?: number;
}

export interface TubeMapLayoutNode extends TubeMapNode {
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
  readonly lane: number;
  readonly label: string;
  readonly labelFontSize: number;
  readonly labelMode: "full" | "compact" | "abbreviated" | "hidden";
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
  readonly portPath: string;
  readonly portOffset: number;
  readonly thickness: number;
  readonly portThickness: number;
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
const PATTERN_PORT_SPREAD = 10;
const MIN_PATTERN_PORT_INSET = 3;
const MAX_PATTERN_PORT_INSET = 5;

/** Deterministic reference-anchored SVG layout with no force simulation. */
export function layoutTubeMap(
  model: TubeMapModel,
  options: TubeMapLayoutOptions,
): TubeMapLayout {
  const width = positiveDimension(options.width, "width");
  const height = positiveDimension(options.height, "height");
  const zoom = clamp(options.zoom ?? 1, 0.2, 5);
  const verticalScale = clamp(options.verticalScale ?? 1, 0.75, 1.45);
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
  const baseLaneGap = Math.min(
    68,
    Math.max(26, (height - 110) / (2 * (maxLaneDepth + 1))),
  );
  const availableLaneRadius = Math.max(
    28,
    Math.min(referenceY - 36, height - referenceY - 36),
  );
  const laneGap = Math.min(
    availableLaneRadius / maxLaneDepth,
    baseLaneGap * verticalScale,
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
    const visibleLabel = visibleNodeLabel(node, displayWidth);
    return [
      {
        ...node,
        x,
        y,
        width: displayWidth,
        height: NODE_HEIGHT,
        lane,
        label: visibleLabel?.text ?? ariaLabel,
        labelFontSize: visibleLabel?.fontSize ?? 10,
        labelMode: visibleLabel?.mode ?? "hidden",
        ariaLabel,
        showLabel: visibleLabel !== undefined,
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
    return [
      {
        ...edge,
        lane,
        path: edgePath(from, to, lane, referenceY, verticalScale),
      },
    ];
  });
  const patternLaneGap = PATTERN_LANE_GAP * verticalScale;
  const portInset = clamp(
    MIN_PATTERN_PORT_INSET + zoom,
    MIN_PATTERN_PORT_INSET,
    MAX_PATTERN_PORT_INSET,
  );
  const patterns = model.patterns.flatMap((pattern, index) => {
    const pathNodes = pattern.nodeKeys.flatMap((key) => {
      const node = positioned.get(key);
      return node === undefined ? [] : [node];
    });
    if (pathNodes.length === 0) return [];
    const lane =
      index % 2 === 0 ? -2 - Math.floor(index / 2) : 2 + Math.floor(index / 2);
    const offset = lane * patternLaneGap;
    const portOffset = patternPortOffset(index, model.patterns.length);
    const geometry = patternGeometry(
      pathNodes,
      lane,
      portOffset,
      patternLaneGap,
      portInset,
    );
    const first = pathNodes[0];
    if (first === undefined) return [];
    const thickness = displayPatternThickness(
      patternThickness(pattern.weight),
      zoom,
      model.patterns.length,
    );
    return [
      {
        ...pattern,
        lane,
        path: geometry.path,
        portPath: geometry.portPath,
        portOffset,
        thickness,
        portThickness: Math.max(1, thickness * 0.58),
        labelX: first.x + first.width / 2,
        labelY: first.y + offset + Math.sign(lane) * 10,
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
      nodes.length + edges.length + patterns.length * 4 + tileBoundaries.length,
  };
}

export function nodeWidth(node: Pick<TubeMapNode, "sequenceLength">): number {
  return clamp(18 + Math.log2(node.sequenceLength + 1) * 8, 28, 140);
}

/** Finds the largest lane scale whose rendered graph remains inside the viewport. */
export function fitTubeMapVerticalScale(
  model: TubeMapModel,
  options: TubeMapVerticalFitOptions,
): number {
  const minimum = clamp(options.minimumScale ?? 0.75, 0.75, 1.45);
  const maximum = clamp(options.maximumScale ?? 1.45, minimum, 1.45);
  const padding = clamp(options.verticalPadding ?? 20, 0, options.height / 3);
  const layoutOptions = {
    width: options.width,
    height: options.height,
    ...(options.zoom === undefined ? {} : { zoom: options.zoom }),
    ...(options.panX === undefined ? {} : { panX: options.panX }),
    ...(options.showBases === undefined
      ? {}
      : { showBases: options.showBases }),
  } satisfies TubeMapLayoutOptions;
  const fits = (verticalScale: number): boolean => {
    const bounds = tubeMapVerticalBounds(
      layoutTubeMap(model, { ...layoutOptions, verticalScale }),
    );
    return bounds.top >= padding && bounds.bottom <= options.height - padding;
  };
  if (fits(maximum)) return maximum;
  if (!fits(minimum)) return minimum;

  let lower = minimum;
  let upper = maximum;
  for (let iteration = 0; iteration < 12; iteration += 1) {
    const candidate = (lower + upper) / 2;
    if (fits(candidate)) lower = candidate;
    else upper = candidate;
  }
  return Math.floor(lower * 1_000) / 1_000;
}

function tubeMapVerticalBounds(layout: TubeMapLayout): {
  readonly top: number;
  readonly bottom: number;
} {
  let top = layout.referenceY - 18;
  let bottom = layout.referenceY + 18;
  const include = (minimum: number, maximum: number): void => {
    top = Math.min(top, minimum);
    bottom = Math.max(bottom, maximum);
  };
  for (const node of layout.nodes)
    include(node.y - node.height / 2 - 3, node.y + node.height / 2 + 3);
  for (const edge of layout.edges) includePathY(edge.path, include);
  for (const pattern of layout.patterns) {
    const margin = pattern.thickness / 2 + 2;
    includePathY(pattern.path, (minimum, maximum) =>
      include(minimum - margin, maximum + margin),
    );
    include(pattern.labelY - 10, pattern.labelY + 5);
  }
  return { top, bottom };
}

function includePathY(
  path: string,
  include: (minimum: number, maximum: number) => void,
): void {
  const values = (path.match(/-?\d+(?:\.\d+)?/g) ?? []).map(Number);
  const yValues: number[] = [];
  for (let index = 1; index < values.length; index += 2) {
    const value = values[index];
    if (value !== undefined) yValues.push(value);
  }
  if (yValues.length > 0) include(Math.min(...yValues), Math.max(...yValues));
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
  verticalScale: number,
): string {
  const startX = from.x + from.width;
  const endX = to.x;
  const startY = from.y;
  const endY = to.y;
  if (from.lane === 0 && to.lane === 0 && lane !== 0) {
    const archY = referenceY + lane * 52 * verticalScale;
    return `M ${round(startX)} ${round(startY)} C ${round(startX + 28)} ${round(archY)}, ${round(endX - 28)} ${round(archY)}, ${round(endX)} ${round(endY)}`;
  }
  const midpoint = (startX + endX) / 2;
  return `M ${round(startX)} ${round(startY)} C ${round(midpoint)} ${round(startY)}, ${round(midpoint)} ${round(endY)}, ${round(endX)} ${round(endY)}`;
}

function patternPortOffset(index: number, count: number): number {
  if (count <= 1) return 0;
  return -PATTERN_PORT_SPREAD + (index / (count - 1)) * PATTERN_PORT_SPREAD * 2;
}

function patternGeometry(
  nodes: readonly TubeMapLayoutNode[],
  lane: number,
  portOffset: number,
  laneGap: number,
  portInset: number,
): { readonly path: string; readonly portPath: string } {
  const first = nodes[0];
  if (first === undefined) return { path: "", portPath: "" };
  const firstCenterX = first.x + first.width / 2;
  let path = `M ${round(firstCenterX)} ${round(first.y + portOffset)}`;
  const ports = new Map<string, string>();
  if (nodes.length === 1) {
    addPatternPort(ports, first, "left", first.y + portOffset, portInset);
    addPatternPort(ports, first, "right", first.y + portOffset, portInset);
  }

  for (let index = 1; index < nodes.length; index += 1) {
    const previous = nodes[index - 1];
    const node = nodes[index];
    if (previous === undefined || node === undefined) continue;
    const previousCenterX = previous.x + previous.width / 2;
    const nodeCenterX = node.x + node.width / 2;
    const direction = nodeCenterX >= previousCenterX ? 1 : -1;
    const fromX = direction > 0 ? previous.x + previous.width : previous.x;
    const toX = direction > 0 ? node.x : node.x + node.width;
    const fromY = previous.y + portOffset;
    const toY = node.y + portOffset;
    path += ` L ${round(fromX)} ${round(fromY)}`;
    path += patternConnection(fromX, fromY, toX, toY, direction, lane, laneGap);
    path += ` L ${round(nodeCenterX)} ${round(toY)}`;
    addPatternPort(
      ports,
      previous,
      direction > 0 ? "right" : "left",
      fromY,
      portInset,
    );
    addPatternPort(
      ports,
      node,
      direction > 0 ? "left" : "right",
      toY,
      portInset,
    );
  }

  return { path, portPath: [...ports.values()].join(" ") };
}

function patternConnection(
  fromX: number,
  fromY: number,
  toX: number,
  toY: number,
  direction: 1 | -1,
  lane: number,
  laneGap: number,
): string {
  const gap = Math.max(0, (toX - fromX) * direction);
  if (gap < 12) {
    const midpoint = (fromX + toX) / 2;
    return ` C ${round(midpoint)} ${round(fromY)}, ${round(midpoint)} ${round(toY)}, ${round(toX)} ${round(toY)}`;
  }
  const lead = Math.min(18, gap / 3);
  const fromLaneX = fromX + direction * lead;
  const toLaneX = toX - direction * lead;
  const laneY = (fromY + toY) / 2 + lane * laneGap;
  return ` C ${round(fromLaneX)} ${round(fromY)}, ${round(fromLaneX)} ${round(laneY)}, ${round(fromLaneX)} ${round(laneY)} L ${round(toLaneX)} ${round(laneY)} C ${round(toLaneX)} ${round(laneY)}, ${round(toLaneX)} ${round(toY)}, ${round(toX)} ${round(toY)}`;
}

function addPatternPort(
  ports: Map<string, string>,
  node: TubeMapLayoutNode,
  side: "left" | "right",
  y: number,
  inset: number,
): void {
  const key = `${node.key}:${side}`;
  if (ports.has(key)) return;
  const centerX = node.x + node.width / 2;
  const edgeX = side === "left" ? node.x : node.x + node.width;
  const innerX =
    side === "left"
      ? Math.min(centerX, edgeX + inset)
      : Math.max(centerX, edgeX - inset);
  ports.set(
    key,
    `M ${round(edgeX)} ${round(y)} L ${round(innerX)} ${round(y)}`,
  );
}

function displayPatternThickness(
  base: number,
  zoom: number,
  patternCount: number,
): number {
  const zoomScale = Math.min(1, Math.sqrt(zoom));
  const densityScale = 1 / (1 + Math.max(0, patternCount - 1) * 0.045);
  return clamp(base * zoomScale * densityScale, 1.35, base);
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
):
  | {
      readonly text: string;
      readonly fontSize: number;
      readonly mode: "full" | "compact" | "abbreviated";
    }
  | undefined {
  const full = nodeLabel(node);
  const fullLabel = fittingLabel(displayWidth, full, [10, 9, 8, 7]);
  if (fullLabel !== undefined) return { ...fullLabel, mode: "full" };
  const members = node.collapsedMembers?.length;
  if (members !== undefined) {
    const compact = fittingLabel(displayWidth, `${members} nodes`, [9, 8, 7]);
    if (compact !== undefined) return { ...compact, mode: "compact" };
    const count = fittingLabel(displayWidth, `${members}×`, [8, 7, 6.5]);
    return count === undefined ? undefined : { ...count, mode: "abbreviated" };
  }

  const identifier = node.id.toString();
  for (
    let suffixLength = Math.min(5, identifier.length - 1);
    suffixLength >= 2;
    suffixLength -= 1
  ) {
    const abbreviated = fittingLabel(
      displayWidth,
      `…${identifier.slice(-suffixLength)}`,
      [8, 7, 6.5],
    );
    if (abbreviated !== undefined)
      return { ...abbreviated, mode: "abbreviated" };
  }
  return undefined;
}

function fittingLabel(
  displayWidth: number,
  text: string,
  fontSizes: readonly number[],
): { readonly text: string; readonly fontSize: number } | undefined {
  const fontSize = fontSizes.find(
    (candidate) => displayWidth >= estimatedLabelWidth(text, candidate),
  );
  return fontSize === undefined ? undefined : { text, fontSize };
}

function estimatedLabelWidth(label: string, fontSize: number): number {
  return label.length * fontSize * 0.61 + 8;
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
