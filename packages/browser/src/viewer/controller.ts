import type { QueryTrace, RegionQuery } from "../reader/types.js";
import type { ViewerDisplayMode } from "./lod.js";
import {
  emptyViewerCounts,
  hitTestEdge,
  hitTestNode,
  hitTestTraversal,
  layoutViewerModel,
  ViewerModelBuilder,
  viewerBudgets,
  viewerSummary,
} from "./model.js";
import { ProgressiveTileQuery } from "./query-controller.js";
import { renderViewerCanvas } from "./renderer.js";
import type {
  PangenomeViewer,
  PangenomeViewerOptions,
  ViewerEventMap,
  ViewerLayerState,
  ViewerLayout,
  ViewerModel,
  ViewerPerformanceSnapshot,
  ViewerSelectionDetail,
  ViewerSnapshot,
  ViewerTheme,
} from "./types.js";

const CANVAS_HEIGHT = 460;
const DEFAULT_LAYERS: ViewerLayerState = {
  reference: true,
  topology: true,
  traversals: true,
  tileBoundaries: true,
  sequenceLabels: true,
};

export function createViewerController(
  container: HTMLElement,
  options: PangenomeViewerOptions,
): PangenomeViewer {
  if (!(container instanceof HTMLElement)) {
    throw new TypeError(
      "createPangenomeViewer requires an HTMLElement container",
    );
  }

  const root = document.createElement("section");
  const canvas = document.createElement("canvas");
  const status = document.createElement("div");
  const countsElement = document.createElement("span");
  const detailElement = document.createElement("span");
  const transformElement = document.createElement("span");
  const traceElement = document.createElement("div");
  const liveElement = document.createElement("div");
  configureElements(
    root,
    canvas,
    status,
    countsElement,
    detailElement,
    transformElement,
    traceElement,
    liveElement,
  );
  status.append(countsElement, detailElement, transformElement);
  root.append(canvas, status, traceElement, liveElement);
  container.append(root);

  const context = canvas.getContext("2d");
  if (context === null) {
    root.remove();
    throw new Error("Canvas 2D is unavailable in this browser");
  }

  const budgets = viewerBudgets({
    ...(options.maxRenderedNodes === undefined
      ? {}
      : { maxRenderedNodes: options.maxRenderedNodes }),
    ...(options.maxRenderedEdges === undefined
      ? {}
      : { maxRenderedEdges: options.maxRenderedEdges }),
    ...(options.maxHaplotypeLanes === undefined
      ? {}
      : { maxHaplotypeLanes: options.maxHaplotypeLanes }),
  });
  const queryController = new ProgressiveTileQuery(options.archive);
  const listeners: {
    [K in keyof ViewerEventMap]: Set<(detail: ViewerEventMap[K]) => void>;
  } = {
    regionchange: new Set(),
    progress: new Set(),
    querytrace: new Set(),
    error: new Set(),
    viewportchange: new Set(),
    selectionchange: new Set(),
    lodchange: new Set(),
  };

  let destroyed = false;
  let loading = false;
  let currentRegion: Readonly<RegionQuery> | undefined;
  let builder: ViewerModelBuilder | undefined;
  let model: ViewerModel | undefined;
  let layout: ViewerLayout | undefined;
  let trace: QueryTrace | undefined;
  let selectedNodeId: bigint | undefined;
  let hoveredNodeId: bigint | undefined;
  let zoom = 1;
  let panX = 0;
  let resetTransformOnNextTile = false;
  let displayMode: ViewerDisplayMode = options.initialDisplayMode ?? "detailed";
  let layers: ViewerLayerState = {
    ...DEFAULT_LAYERS,
    ...options.initialLayers,
  };
  let theme: ViewerTheme = options.initialTheme ?? "light";
  let cssWidth = 900;
  let cssHeight = CANVAS_HEIGHT;
  let frame: number | undefined;
  let pointerId: number | undefined;
  const activePointers = new Map<number, { x: number; y: number }>();
  let pointerStartX = 0;
  let pointerStartPan = 0;
  let pointerMoved = false;
  let pinchStartDistance = 0;
  let pinchStartZoom = 1;
  let pinchWorldX = 0;
  let viewportTimer: ReturnType<typeof globalThis.setTimeout> | undefined;
  let queryStartedAt = 0;
  let modelUpdateMs = 0;
  let layoutMs = 0;
  let paintMs = 0;
  let firstTilePaintMs: number | undefined;
  let queryCompleteMs: number | undefined;
  const frameDurations: number[] = [];

  const emit = <K extends keyof ViewerEventMap>(
    event: K,
    detail: ViewerEventMap[K],
  ): void => {
    for (const listener of listeners[event]) listener(detail);
  };

  const updateStatus = (): void => {
    const counts = model?.counts ?? emptyViewerCounts();
    countsElement.textContent =
      `${counts.tiles} tile${counts.tiles === 1 ? "" : "s"} · ` +
      `${counts.renderedNodes}/${counts.decodedNodes} nodes · ` +
      `${counts.renderedEdges}/${counts.decodedEdges} edges · ` +
      `${counts.renderedHaplotypeLanes}/${counts.decodedTraversals} local traversals`;
    transformElement.textContent = `Zoom ${zoom.toFixed(2)}×`;
    transformElement.dataset.viewerZoom = zoom.toFixed(3);
    const summary = viewerSummary(counts);
    liveElement.textContent =
      summary ?? (loading ? "Loading region" : "Region ready");
    if (options.showRequestTrace === true && trace !== undefined) {
      traceElement.hidden = false;
      traceElement.textContent =
        `${trace.requestRanges.length} range reads · ${formatBytes(trace.totalBytes)} · ` +
        `${trace.dependencyRounds} rounds · decode ${trace.decodeMs.toFixed(1)} ms · ` +
        `decompress ${trace.decompressionMs.toFixed(1)} ms · ${trace.canonicalHash.slice(0, 12)}`;
    } else {
      traceElement.hidden = true;
    }
    canvas.setAttribute(
      "aria-label",
      currentRegion === undefined
        ? "Pangenome graph viewer, no region loaded"
        : `Pangenome graph for ${currentRegion.sample} ${currentRegion.contig} ` +
            `${currentRegion.start} to ${currentRegion.end}; ${counts.renderedNodes} rendered nodes; ` +
            "anonymous weighted traversals are tile-local and are not named individuals",
    );
  };

  const render = (): void => {
    frame = undefined;
    if (destroyed || model === undefined) {
      updateStatus();
      return;
    }
    const layoutStarted = performance.now();
    layout = layoutViewerModel(model, {
      width: cssWidth,
      height: cssHeight,
      zoom,
      panX,
    });
    layoutMs += performance.now() - layoutStarted;
    const ratio = Math.max(1, globalThis.devicePixelRatio ?? 1);
    context.setTransform(ratio, 0, 0, ratio, 0, 0);
    const paintStarted = performance.now();
    renderViewerCanvas(context, layout, {
      ...(options.background === undefined
        ? {}
        : { background: options.background }),
      ...(hoveredNodeId === undefined ? {} : { hoveredNodeId }),
      ...(selectedNodeId === undefined ? {} : { selectedNodeId }),
      loading,
      displayMode,
      layers,
      theme,
    });
    paintMs += performance.now() - paintStarted;
    frameDurations.push(performance.now() - layoutStarted);
    if (frameDurations.length > 120) frameDurations.shift();
    if (
      firstTilePaintMs === undefined &&
      queryStartedAt > 0 &&
      layout.counts.tiles > 0
    ) {
      firstTilePaintMs = performance.now() - queryStartedAt;
    }
    updateStatus();
  };

  const scheduleRender = (): void => {
    if (frame !== undefined || destroyed) return;
    frame = requestFrame(render);
  };

  const resize = (): void => {
    if (destroyed) return;
    cssWidth = Math.max(320, root.clientWidth || container.clientWidth || 900);
    cssHeight = Math.max(280, canvas.clientHeight || CANVAS_HEIGHT);
    const ratio = Math.max(1, globalThis.devicePixelRatio ?? 1);
    canvas.width = Math.round(cssWidth * ratio);
    canvas.height = Math.round(cssHeight * ratio);
    scheduleRender();
  };

  const setRegion = async (region: RegionQuery): Promise<void> => {
    if (destroyed) throw new Error("pangenome viewer is destroyed");
    try {
      await queryController.run(region, {
        onStart: (startedRegion) => {
          queryStartedAt = performance.now();
          modelUpdateMs = 0;
          layoutMs = 0;
          paintMs = 0;
          firstTilePaintMs = undefined;
          queryCompleteMs = undefined;
          frameDurations.length = 0;
          currentRegion = startedRegion;
          const preserveRenderedRegion = (model?.counts.tiles ?? 0) > 0;
          builder = new ViewerModelBuilder(region, budgets);
          if (!preserveRenderedRegion) model = builder.snapshot();
          trace = undefined;
          selectedNodeId = undefined;
          hoveredNodeId = undefined;
          emit("selectionchange", undefined);
          resetTransformOnNextTile =
            preserveRenderedRegion && (zoom !== 1 || panX !== 0);
          if (!preserveRenderedRegion) {
            zoom = 1;
            panX = 0;
          }
          loading = true;
          detailElement.textContent =
            "anonymous weighted traversals are local evidence, not named individuals";
          emit("regionchange", startedRegion);
          scheduleRender();
        },
        onTile: (tile) => {
          const modelStarted = performance.now();
          builder?.addTile(tile);
          model = builder?.snapshot();
          if (resetTransformOnNextTile) {
            zoom = 1;
            panX = 0;
            resetTransformOnNextTile = false;
          }
          modelUpdateMs += performance.now() - modelStarted;
          if (model === undefined || currentRegion === undefined) return;
          const summary = viewerSummary(model.counts);
          emit("progress", {
            region: currentRegion,
            counts: model.counts,
            summarized: summary !== undefined,
            ...(summary === undefined ? {} : { summary }),
          });
          scheduleRender();
        },
        onTrace: (queryTrace) => {
          trace = queryTrace;
          emit("querytrace", queryTrace);
          scheduleRender();
        },
        onComplete: () => {
          if (resetTransformOnNextTile) {
            zoom = 1;
            panX = 0;
            resetTransformOnNextTile = false;
          }
          queryCompleteMs = performance.now() - queryStartedAt;
          loading = false;
          render();
        },
      });
    } catch (cause) {
      resetTransformOnNextTile = false;
      loading = false;
      const error = toError(cause);
      liveElement.textContent = error.message;
      emit("error", {
        error,
        ...(currentRegion === undefined ? {} : { region: currentRegion }),
      });
      render();
      throw error;
    }
  };

  const scheduleViewportChange = (
    source: "pointer" | "keyboard" | "api",
  ): void => {
    if (viewportTimer !== undefined) globalThis.clearTimeout(viewportTimer);
    viewportTimer = globalThis.setTimeout(() => {
      viewportTimer = undefined;
      if (destroyed || currentRegion === undefined) return;
      const plotWidth = Math.max(1, cssWidth - 78);
      emit("viewportchange", {
        visualRegion: transformedRegion(currentRegion, zoom, panX, plotWidth),
        source,
      });
    }, 180);
  };

  const pointerPosition = (
    event: PointerEvent | WheelEvent,
  ): [number, number] => {
    const rect = canvas.getBoundingClientRect();
    return [event.clientX - rect.left, event.clientY - rect.top];
  };

  const onWheel = (event: WheelEvent): void => {
    event.preventDefault();
    const [x] = pointerPosition(event);
    const previousZoom = zoom;
    zoom = clamp(zoom * Math.exp(-event.deltaY * 0.0015), 0.5, 24);
    const world = (x - 54 - panX) / previousZoom;
    panX = x - 54 - world * zoom;
    scheduleRender();
    scheduleViewportChange("pointer");
  };
  const onPointerDown = (event: PointerEvent): void => {
    activePointers.set(event.pointerId, { x: event.clientX, y: event.clientY });
    pointerId = event.pointerId;
    pointerStartX = event.clientX;
    pointerStartPan = panX;
    pointerMoved = false;
    canvas.setPointerCapture?.(event.pointerId);
    canvas.style.cursor = "grabbing";
    if (activePointers.size === 2) beginPinch();
  };
  const onPointerMove = (event: PointerEvent): void => {
    const [x, y] = pointerPosition(event);
    if (activePointers.has(event.pointerId)) {
      activePointers.set(event.pointerId, {
        x: event.clientX,
        y: event.clientY,
      });
    }
    if (activePointers.size === 2 && pinchStartDistance > 0) {
      const [first, second] = [...activePointers.values()];
      if (first === undefined || second === undefined) return;
      const distance = Math.hypot(first.x - second.x, first.y - second.y);
      const rect = canvas.getBoundingClientRect();
      const centerX = (first.x + second.x) / 2 - rect.left;
      zoom = clamp(pinchStartZoom * (distance / pinchStartDistance), 0.5, 24);
      panX = centerX - 54 - pinchWorldX * zoom;
      pointerMoved = true;
      scheduleRender();
      return;
    }
    if (pointerId === event.pointerId) {
      const movement = event.clientX - pointerStartX;
      pointerMoved ||= Math.abs(movement) > 3;
      panX = pointerStartPan + movement;
      scheduleRender();
      return;
    }
    const hovered =
      layout === undefined ? undefined : hitTestNode(layout, x, y);
    if (hovered?.id === hoveredNodeId) return;
    hoveredNodeId = hovered?.id;
    detailElement.textContent =
      hovered === undefined
        ? "anonymous weighted traversals are local evidence, not named individuals"
        : nodeDescription(hovered);
    canvas.style.cursor = hovered === undefined ? "grab" : "pointer";
    scheduleRender();
  };
  const onPointerUp = (event: PointerEvent): void => {
    if (!activePointers.has(event.pointerId)) return;
    const [x, y] = pointerPosition(event);
    const wasPinching = activePointers.size > 1;
    if (!pointerMoved && !wasPinching && layout !== undefined) {
      const selected = selectionAt(layout, x, y);
      selectedNodeId = selected?.kind === "node" ? selected.node.id : undefined;
      if (selected !== undefined)
        detailElement.textContent = selectionDescription(selected);
      emit("selectionchange", selected);
    }
    canvas.releasePointerCapture?.(event.pointerId);
    activePointers.delete(event.pointerId);
    const remaining = activePointers.entries().next().value as
      | [number, { x: number; y: number }]
      | undefined;
    pointerId = remaining?.[0];
    if (remaining !== undefined) {
      pointerStartX = remaining[1].x;
      pointerStartPan = panX;
      pointerMoved = true;
    }
    pinchStartDistance = 0;
    canvas.style.cursor = "grab";
    scheduleRender();
    if (pointerMoved && activePointers.size === 0)
      scheduleViewportChange("pointer");
  };
  const onPointerLeave = (): void => {
    if (activePointers.size > 0) return;
    hoveredNodeId = undefined;
    scheduleRender();
  };
  const beginPinch = (): void => {
    const [first, second] = [...activePointers.values()];
    if (first === undefined || second === undefined) return;
    pinchStartDistance = Math.max(
      1,
      Math.hypot(first.x - second.x, first.y - second.y),
    );
    pinchStartZoom = zoom;
    const rect = canvas.getBoundingClientRect();
    const centerX = (first.x + second.x) / 2 - rect.left;
    pinchWorldX = (centerX - 54 - panX) / zoom;
    pointerMoved = true;
  };
  const onDoubleClick = (): void => {
    zoom = 1;
    panX = 0;
    scheduleRender();
  };
  const onKeyDown = (event: KeyboardEvent): void => {
    let handled = true;
    switch (event.key) {
      case "+":
      case "=":
        zoom = clamp(zoom * 1.25, 0.5, 24);
        break;
      case "-":
        zoom = clamp(zoom / 1.25, 0.5, 24);
        break;
      case "ArrowLeft":
        panX += 40;
        break;
      case "ArrowRight":
        panX -= 40;
        break;
      case "Home":
      case "0":
        zoom = 1;
        panX = 0;
        break;
      default:
        handled = false;
    }
    if (handled) {
      event.preventDefault();
      scheduleRender();
      scheduleViewportChange("keyboard");
    }
  };

  canvas.addEventListener("wheel", onWheel, { passive: false });
  canvas.addEventListener("pointerdown", onPointerDown);
  canvas.addEventListener("pointermove", onPointerMove);
  canvas.addEventListener("pointerup", onPointerUp);
  canvas.addEventListener("pointercancel", onPointerUp);
  canvas.addEventListener("pointerleave", onPointerLeave);
  canvas.addEventListener("dblclick", onDoubleClick);
  canvas.addEventListener("keydown", onKeyDown);
  const resizeObserver =
    typeof ResizeObserver === "undefined"
      ? undefined
      : new ResizeObserver(resize);
  resizeObserver?.observe(container);
  resize();

  const viewer: PangenomeViewer = {
    setRegion,
    setViewport: setRegion,
    getRegion: () => currentRegion,
    getSnapshot: (): ViewerSnapshot => ({
      ...(currentRegion === undefined ? {} : { region: currentRegion }),
      counts: model?.counts ?? emptyViewerCounts(),
      loading,
      zoom,
      panX,
      displayMode,
      layers,
      theme,
      ...(selectedNodeId === undefined ? {} : { selectedNodeId }),
      ...(trace === undefined ? {} : { trace }),
    }),
    resize,
    zoomBy: (factor) => {
      if (!Number.isFinite(factor) || factor <= 0) {
        throw new RangeError("viewer zoom factor must be positive and finite");
      }
      zoom = clamp(zoom * factor, 0.5, 24);
      scheduleRender();
      scheduleViewportChange("api");
    },
    panBy: (pixels) => {
      if (!Number.isFinite(pixels)) {
        throw new RangeError("viewer pan distance must be finite");
      }
      panX += pixels;
      scheduleRender();
      scheduleViewportChange("api");
    },
    resetView: () => {
      zoom = 1;
      panX = 0;
      scheduleRender();
    },
    setDisplayMode: (mode) => {
      if (!["overview", "regional", "detailed", "base"].includes(mode)) {
        throw new TypeError(`unsupported viewer display mode ${mode}`);
      }
      if (displayMode === mode) return;
      displayMode = mode;
      emit("lodchange", mode);
      scheduleRender();
    },
    setLayers: (nextLayers) => {
      layers = { ...layers, ...nextLayers };
      scheduleRender();
    },
    setTheme: (nextTheme) => {
      if (nextTheme !== "light" && nextTheme !== "dark") {
        throw new TypeError(`unsupported viewer theme ${nextTheme}`);
      }
      theme = nextTheme;
      applyElementTheme(root, status, traceElement, theme);
      scheduleRender();
    },
    getPerformanceSnapshot: (): ViewerPerformanceSnapshot => ({
      modelUpdateMs,
      layoutMs,
      paintMs,
      ...(firstTilePaintMs === undefined ? {} : { firstTilePaintMs }),
      ...(queryCompleteMs === undefined ? {} : { queryCompleteMs }),
      frameP95Ms: percentile(frameDurations, 0.95),
      sampledFrames: frameDurations.length,
    }),
    on: (event, listener) => {
      listeners[event].add(listener);
      return () => listeners[event].delete(listener);
    },
    destroy: () => {
      if (destroyed) return;
      destroyed = true;
      queryController.destroy();
      resizeObserver?.disconnect();
      if (frame !== undefined) cancelFrame(frame);
      if (viewportTimer !== undefined) globalThis.clearTimeout(viewportTimer);
      canvas.removeEventListener("wheel", onWheel);
      canvas.removeEventListener("pointerdown", onPointerDown);
      canvas.removeEventListener("pointermove", onPointerMove);
      canvas.removeEventListener("pointerup", onPointerUp);
      canvas.removeEventListener("pointercancel", onPointerUp);
      canvas.removeEventListener("pointerleave", onPointerLeave);
      canvas.removeEventListener("dblclick", onDoubleClick);
      canvas.removeEventListener("keydown", onKeyDown);
      for (const listenerSet of Object.values(listeners)) listenerSet.clear();
      root.remove();
    },
  };

  applyElementTheme(root, status, traceElement, theme);

  if (options.initialRegion !== undefined) {
    void viewer.setRegion(options.initialRegion).catch(() => undefined);
  }
  return viewer;
}

/** Resolve a visual transform to the nearest non-empty integer genomic span. */
export function transformedRegion(
  region: Readonly<RegionQuery>,
  zoom: number,
  panX: number,
  plotWidth: number,
): RegionQuery {
  const interval = region.end - region.start;
  const safeZoom = Math.max(Number.EPSILON, zoom);
  const safeWidth = Math.max(1, plotWidth);
  const rawStart = region.start + (-panX / (safeWidth * safeZoom)) * interval;
  const rawEnd =
    region.start + ((safeWidth - panX) / (safeWidth * safeZoom)) * interval;
  const span = Math.max(1, Math.round(rawEnd - rawStart));
  const center = (rawStart + rawEnd) / 2;
  const start = Math.max(0, Math.round(center - span / 2));
  return {
    sample: region.sample,
    contig: region.contig,
    start,
    end: start + span,
    ...(region.context === undefined ? {} : { context: region.context }),
  };
}

function applyElementTheme(
  root: HTMLElement,
  status: HTMLElement,
  trace: HTMLElement,
  theme: ViewerTheme,
): void {
  const dark = theme === "dark";
  root.dataset.viewerTheme = theme;
  root.style.background = dark ? "#10151d" : "#fbfcfe";
  root.style.borderColor = dark ? "#354153" : "#d9e2ec";
  status.style.borderColor = dark ? "#303b4b" : "#e3e9ef";
  status.style.color = dark ? "#aeb9c9" : "#516176";
  trace.style.borderColor = dark ? "#303b4b" : "#e3e9ef";
  trace.style.color = dark ? "#aeb9c9" : "#516176";
}

function configureElements(
  root: HTMLElement,
  canvas: HTMLCanvasElement,
  status: HTMLElement,
  counts: HTMLElement,
  details: HTMLElement,
  transform: HTMLElement,
  trace: HTMLElement,
  live: HTMLElement,
): void {
  root.dataset.pangenomeViewer = "true";
  Object.assign(root.style, {
    width: "100%",
    overflow: "hidden",
    border: "1px solid #d9e2ec",
    borderRadius: "14px",
    background: "#fbfcfe",
    boxShadow: "0 12px 34px rgba(20, 33, 61, 0.08)",
  });
  canvas.dataset.viewerCanvas = "true";
  canvas.tabIndex = 0;
  canvas.setAttribute("role", "img");
  Object.assign(canvas.style, {
    display: "block",
    width: "100%",
    height: `${CANVAS_HEIGHT}px`,
    cursor: "grab",
    touchAction: "none",
    outlineOffset: "-3px",
  });
  Object.assign(status.style, {
    display: "grid",
    gridTemplateColumns: "minmax(0, 1fr) minmax(0, 1.3fr) auto",
    gap: "14px",
    padding: "10px 14px",
    borderTop: "1px solid #e3e9ef",
    color: "#516176",
    font: "12px/1.4 ui-monospace, SFMono-Regular, Menlo, monospace",
  });
  details.style.textAlign = "center";
  details.dataset.viewerDetail = "true";
  transform.style.whiteSpace = "nowrap";
  Object.assign(trace.style, {
    padding: "8px 14px",
    borderTop: "1px solid #e3e9ef",
    color: "#516176",
    font: "12px/1.4 ui-monospace, SFMono-Regular, Menlo, monospace",
  });
  live.setAttribute("role", "status");
  live.setAttribute("aria-live", "polite");
  Object.assign(live.style, {
    position: "absolute",
    width: "1px",
    height: "1px",
    overflow: "hidden",
    clip: "rect(0 0 0 0)",
  });
  counts.textContent = "No region loaded";
  details.textContent =
    "Scroll to zoom · drag to pan · select a node for details";
  transform.textContent = "Zoom 1.00×";
}

function nodeDescription(node: {
  id: bigint;
  sequenceLength: number;
  reference: boolean;
  reverse: boolean;
}): string {
  return (
    `node ${node.id.toString()} · ${node.sequenceLength} bp · ` +
    `${node.reference ? "reference" : "alternate"} · ` +
    `${node.reverse ? "reverse" : "forward"}`
  );
}

function selectionAt(
  layout: ViewerLayout,
  x: number,
  y: number,
): ViewerSelectionDetail | undefined {
  const node = hitTestNode(layout, x, y);
  if (node !== undefined) {
    return {
      kind: "node",
      node,
      incoming: layout.edges.filter((edge) => edge.to === node.id),
      outgoing: layout.edges.filter((edge) => edge.from === node.id),
      localTraversalWeights: layout.traversals.flatMap((traversal) =>
        traversal.orientedNodes.some((handle) => handle >> 1n === node.id)
          ? [
              {
                tileStart: traversal.tileStart,
                tileEnd: traversal.tileEnd,
                weight: traversal.weight,
              },
            ]
          : [],
      ),
    };
  }
  const traversal = hitTestTraversal(layout, x, y);
  if (traversal !== undefined) return { kind: "traversal", traversal };
  const edge = hitTestEdge(layout, x, y);
  return edge === undefined ? undefined : { kind: "edge", edge };
}

function selectionDescription(selection: ViewerSelectionDetail): string {
  if (selection.kind === "node") return nodeDescription(selection.node);
  if (selection.kind === "edge") {
    return `edge ${selection.edge.from.toString()} → ${selection.edge.to.toString()} · ${selection.edge.classification}`;
  }
  return (
    `tile-local traversal · weight ${selection.traversal.weight.toString()} · ` +
    `${selection.traversal.tileStart}–${selection.traversal.tileEnd}`
  );
}

function formatBytes(bytes: number): string {
  if (bytes < 1_024) return `${bytes} B`;
  if (bytes < 1_048_576) return `${(bytes / 1_024).toFixed(1)} KiB`;
  return `${(bytes / 1_048_576).toFixed(1)} MiB`;
}

function toError(cause: unknown): Error {
  return cause instanceof Error ? cause : new Error(String(cause));
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(minimum, value));
}

function percentile(values: readonly number[], fraction: number): number {
  if (values.length === 0) return 0;
  const sorted = [...values].sort((left, right) => left - right);
  const index = Math.min(
    sorted.length - 1,
    Math.max(0, Math.ceil(sorted.length * fraction) - 1),
  );
  return sorted[index] ?? 0;
}

function requestFrame(callback: () => void): number {
  if (typeof requestAnimationFrame === "function") {
    return requestAnimationFrame(callback);
  }
  return globalThis.setTimeout(callback, 16) as unknown as number;
}

function cancelFrame(frame: number): void {
  if (typeof cancelAnimationFrame === "function") cancelAnimationFrame(frame);
  else globalThis.clearTimeout(frame);
}
