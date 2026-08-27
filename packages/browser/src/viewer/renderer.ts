import type { ViewerDisplayMode } from "./lod.js";
import type {
  ViewerLayerState,
  ViewerLayout,
  ViewerLayoutNode,
  ViewerTheme,
} from "./types.js";

export interface CanvasRenderOptions {
  readonly background?: string;
  readonly hoveredNodeId?: bigint;
  readonly selectedNodeId?: bigint;
  readonly loading?: boolean;
  readonly displayMode?: ViewerDisplayMode;
  readonly layers?: ViewerLayerState;
  readonly theme?: ViewerTheme;
}

const LIGHT_COLORS = {
  ink: "#14213d",
  muted: "#66758c",
  grid: "#d9e2ec",
  reference: "#147d92",
  referenceFill: "#d8f3f5",
  alternate: "#c65d34",
  alternateFill: "#fff1e8",
  traversal: "#7562a8",
  selection: "#f2b134",
  tile: "#94a3b8",
};

const DARK_COLORS: ViewerColors = {
  ink: "#e8eef7",
  muted: "#9eacbf",
  grid: "#354153",
  reference: "#58c9d7",
  referenceFill: "#173b45",
  alternate: "#f1956f",
  alternateFill: "#43291f",
  traversal: "#b7a7ed",
  selection: "#ffd166",
  tile: "#718096",
};

type ViewerColors = typeof LIGHT_COLORS;

/** Draw a complete layout without retaining DOM or archive state. */
export function renderViewerCanvas(
  context: CanvasRenderingContext2D,
  layout: ViewerLayout,
  options: CanvasRenderOptions = {},
): void {
  const colors = options.theme === "dark" ? DARK_COLORS : LIGHT_COLORS;
  const background =
    options.background ?? (options.theme === "dark" ? "#10151d" : "#fbfcfe");
  context.save();
  context.clearRect(0, 0, layout.width, layout.height);
  context.fillStyle = background;
  context.fillRect(0, 0, layout.width, layout.height);
  context.lineCap = "round";
  context.lineJoin = "round";
  context.font = "12px ui-monospace, SFMono-Regular, Menlo, monospace";

  drawRuler(context, layout, colors);
  const layers = options.layers ?? {
    reference: true,
    topology: true,
    traversals: true,
    tileBoundaries: true,
    sequenceLabels: true,
  };
  if (layers.tileBoundaries) drawTileBoundaries(context, layout, colors);
  const aggregateTopology =
    options.displayMode !== "base" &&
    layout.nodes.filter((node) => node.visible).length > 320;
  if (layers.topology) {
    if (aggregateTopology) {
      drawBundledEdges(context, layout, colors);
    } else {
      drawEdges(context, layout, colors);
    }
  }
  if (layers.traversals) drawTraversals(context, layout, colors);
  drawNodes(context, layout, options, layers, colors, aggregateTopology);
  drawLegend(context, layout, options.loading ?? false, colors);
  context.restore();
}

function drawBundledEdges(
  context: CanvasRenderingContext2D,
  layout: ViewerLayout,
  colors: ViewerColors,
): void {
  drawEdges(
    context,
    { ...layout, edges: layout.edges.filter((edge) => edge.reference) },
    colors,
  );
  const bundles = new Map<
    string,
    { fromX: number; toX: number; count: number; inversion: boolean }
  >();
  for (const edge of layout.edges) {
    if (edge.reference) continue;
    const fromX = Math.min(edge.fromX, edge.toX);
    const toX = Math.max(edge.fromX, edge.toX);
    const key = `${Math.round(fromX / 36)}:${Math.round(toX / 36)}:${edge.classification}`;
    const existing = bundles.get(key);
    if (existing === undefined) {
      bundles.set(key, {
        fromX,
        toX,
        count: 1,
        inversion: edge.classification === "inversion",
      });
    } else {
      existing.fromX = Math.min(existing.fromX, fromX);
      existing.toX = Math.max(existing.toX, toX);
      existing.count += 1;
    }
  }
  const visible = [...bundles.values()]
    .sort((left, right) => right.count - left.count)
    .slice(0, 10)
    .sort((left, right) => left.fromX - right.fromX);
  const backboneY = Math.round(layout.height * 0.42) + 11;
  context.save();
  for (const [index, bundle] of visible.entries()) {
    const width = Math.max(24, bundle.toX - bundle.fromX);
    const lane = index % 5;
    const controlY = backboneY - 48 - lane * 24;
    context.strokeStyle = bundle.inversion
      ? colors.traversal
      : colors.alternate;
    context.globalAlpha = 0.82;
    context.lineWidth = Math.min(7, 1.8 + Math.log2(bundle.count + 1));
    context.beginPath();
    context.moveTo(bundle.fromX, backboneY);
    context.bezierCurveTo(
      bundle.fromX + width * 0.25,
      controlY,
      bundle.toX - width * 0.25,
      controlY,
      bundle.toX,
      backboneY,
    );
    context.stroke();
    if (bundle.count > 1 && width > 46) {
      const label = `${bundle.count} branches`;
      const x = (bundle.fromX + bundle.toX) / 2;
      context.font = "600 11px -apple-system, BlinkMacSystemFont, sans-serif";
      const labelWidth = context.measureText(label).width + 18;
      context.fillStyle = bundle.inversion
        ? colors.traversal
        : colors.alternate;
      context.globalAlpha = 0.95;
      context.beginPath();
      context.roundRect(x - labelWidth / 2, controlY - 14, labelWidth, 24, 10);
      context.fill();
      context.fillStyle = "#ffffff";
      context.textAlign = "center";
      context.fillText(label, x, controlY + 2);
    }
  }
  context.restore();
}

function drawRuler(
  context: CanvasRenderingContext2D,
  layout: ViewerLayout,
  colors: ViewerColors,
): void {
  const left = 54;
  const right = layout.width - 24;
  const y = 42;
  context.strokeStyle = colors.ink;
  context.lineWidth = 1;
  context.beginPath();
  context.moveTo(left, y);
  context.lineTo(right, y);
  context.stroke();
  const interval = layout.query.end - layout.query.start;
  for (let index = 0; index <= 5; index += 1) {
    const fraction = index / 5;
    const coordinate = Math.round(layout.query.start + interval * fraction);
    const worldX = left + fraction * (right - left) * layout.zoom + layout.panX;
    if (worldX < left - 10 || worldX > layout.width + 10) continue;
    context.beginPath();
    context.moveTo(worldX, y - 4);
    context.lineTo(worldX, y + 5);
    context.stroke();
    context.fillStyle = colors.muted;
    context.textAlign = "center";
    context.fillText(formatCoordinate(coordinate), worldX, y - 9);
  }
  context.fillStyle = colors.ink;
  context.textAlign = "left";
  context.fillText(`${layout.query.sample} / ${layout.query.contig}`, left, 18);
}

function drawTileBoundaries(
  context: CanvasRenderingContext2D,
  layout: ViewerLayout,
  colors: ViewerColors,
): void {
  context.save();
  context.strokeStyle = colors.tile;
  context.globalAlpha = 0.35;
  context.setLineDash([3, 5]);
  for (const boundary of layout.tileBoundaries) {
    if (boundary.x < 45 || boundary.x > layout.width) continue;
    context.beginPath();
    context.moveTo(boundary.x, 56);
    context.lineTo(boundary.x, layout.height - 58);
    context.stroke();
  }
  context.restore();
}

function drawEdges(
  context: CanvasRenderingContext2D,
  layout: ViewerLayout,
  colors: ViewerColors,
): void {
  for (const edge of layout.edges) {
    if (
      Math.max(edge.fromX, edge.toX) < -100 ||
      Math.min(edge.fromX, edge.toX) > layout.width + 100
    ) {
      continue;
    }
    context.strokeStyle =
      edge.classification === "inversion"
        ? colors.traversal
        : edge.reference
          ? colors.reference
          : colors.alternate;
    context.globalAlpha = edge.reference ? 0.78 : 0.5;
    context.lineWidth = edge.reference ? 2.4 : 1.4;
    const bend = Math.max(16, Math.abs(edge.toX - edge.fromX) * 0.16);
    const controlY = Math.min(edge.fromY, edge.toY) - bend;
    context.beginPath();
    context.moveTo(edge.fromX, edge.fromY);
    context.quadraticCurveTo(
      (edge.fromX + edge.toX) / 2,
      controlY,
      edge.toX,
      edge.toY,
    );
    context.stroke();
  }
  context.globalAlpha = 1;
}

function drawTraversals(
  context: CanvasRenderingContext2D,
  layout: ViewerLayout,
  colors: ViewerColors,
): void {
  if (layout.traversals.length === 0) return;
  let maximumWeight = 1n;
  for (const traversal of layout.traversals) {
    if (traversal.weight > maximumWeight) maximumWeight = traversal.weight;
  }
  const panelLeft = 54;
  const panelWidth = Math.min(180, layout.width * 0.22);
  context.fillStyle = colors.muted;
  context.textAlign = "left";
  context.fillText(
    "tile-local traversal weight",
    panelLeft,
    layout.height - 22,
  );
  for (const traversal of layout.traversals) {
    if (traversal.points.length >= 2) {
      context.strokeStyle = colors.traversal;
      context.globalAlpha =
        0.25 + 0.65 * weightRatio(traversal.weight, maximumWeight);
      context.lineWidth = 1 + weightRatio(traversal.weight, maximumWeight) * 2;
      context.beginPath();
      const first = traversal.points[0];
      if (first !== undefined) context.moveTo(first.x, first.y);
      for (const point of traversal.points.slice(1))
        context.lineTo(point.x, point.y);
      context.stroke();
    }
    const barY = layout.height - 44 - traversal.lane * 6;
    if (barY < layout.height * 0.72) continue;
    context.globalAlpha = 0.65;
    context.fillStyle = colors.traversal;
    context.fillRect(
      panelLeft,
      barY,
      Math.max(2, panelWidth * weightRatio(traversal.weight, maximumWeight)),
      3,
    );
  }
  context.globalAlpha = 1;
}

function drawNodes(
  context: CanvasRenderingContext2D,
  layout: ViewerLayout,
  options: CanvasRenderOptions,
  layers: ViewerLayerState,
  colors: ViewerColors,
  aggregateTopology: boolean,
): void {
  for (const node of layout.nodes) {
    if (!node.visible || (node.reference && !layers.reference)) continue;
    const selected = node.id === options.selectedNodeId;
    const hovered = node.id === options.hoveredNodeId;
    if (aggregateTopology && !node.reference && !selected && !hovered) continue;
    if (
      aggregateTopology &&
      node.reference &&
      !selected &&
      !hovered &&
      node.width < 3
    ) {
      continue;
    }
    context.fillStyle = node.reference
      ? colors.referenceFill
      : colors.alternateFill;
    context.strokeStyle = selected
      ? colors.selection
      : node.reference
        ? colors.reference
        : colors.alternate;
    context.lineWidth = selected ? 3 : hovered ? 2.4 : 1.3;
    context.beginPath();
    context.rect(node.x, node.y, node.width, node.height);
    context.fill();
    context.stroke();
    drawOrientation(context, node, colors);
    const labelThreshold = options.displayMode === "base" ? 0.8 : 1.7;
    if (
      layers.sequenceLabels &&
      layout.zoom >= labelThreshold &&
      node.width >= 28
    ) {
      context.fillStyle = colors.ink;
      context.textAlign = "center";
      const label = sequenceLabel(node);
      context.fillText(
        label,
        node.x + node.width / 2,
        node.y + node.height / 2 + 4,
        Math.max(8, node.width - 12),
      );
    }
  }
}

function drawOrientation(
  context: CanvasRenderingContext2D,
  node: ViewerLayoutNode,
  colors: ViewerColors,
): void {
  const x = node.reverse ? node.x + 4 : node.x + node.width - 4;
  const direction = node.reverse ? -1 : 1;
  const y = node.y + node.height / 2;
  context.fillStyle = node.reference ? colors.reference : colors.alternate;
  context.beginPath();
  context.moveTo(x + direction * 3, y);
  context.lineTo(x - direction * 3, y - 3);
  context.lineTo(x - direction * 3, y + 3);
  context.closePath();
  context.fill();
}

function drawLegend(
  context: CanvasRenderingContext2D,
  layout: ViewerLayout,
  loading: boolean,
  colors: ViewerColors,
): void {
  context.fillStyle = colors.muted;
  context.textAlign = "right";
  context.fillText(
    `${layout.counts.renderedNodes}/${layout.counts.decodedNodes} nodes · ` +
      `${layout.counts.renderedEdges}/${layout.counts.decodedEdges} edges` +
      (loading ? " · loading…" : ""),
    layout.width - 20,
    18,
  );
  context.textAlign = "left";
  context.fillStyle = colors.reference;
  context.fillRect(54, 58, 14, 4);
  context.fillStyle = colors.muted;
  context.fillText("reference", 74, 64);
  context.fillStyle = colors.alternate;
  context.fillRect(150, 58, 14, 4);
  context.fillStyle = colors.muted;
  context.fillText("alternate topology", 170, 64);
}

function sequenceLabel(node: ViewerLayoutNode): string {
  if (node.sequence.length === 0) return node.id.toString();
  if (node.sequence.length <= 12) return node.sequence;
  return `${node.sequence.slice(0, 9)}…`;
}

function weightRatio(weight: bigint, maximum: bigint): number {
  if (maximum <= 0n) return 0;
  return Number((weight * 10_000n) / maximum) / 10_000;
}

function formatCoordinate(value: number): string {
  return new Intl.NumberFormat("en-US", { maximumFractionDigits: 0 }).format(
    value,
  );
}
