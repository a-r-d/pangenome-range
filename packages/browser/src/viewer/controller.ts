import type { QueryTrace, RegionQuery } from "../reader/types.js";
import {
  emptyViewerCounts,
  hitTestNode,
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
  ViewerLayout,
  ViewerModel,
  ViewerSnapshot,
} from "./types.js";

const CANVAS_HEIGHT = 460;

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
  let cssWidth = 900;
  let cssHeight = CANVAS_HEIGHT;
  let frame: number | undefined;
  let pointerId: number | undefined;
  let pointerStartX = 0;
  let pointerStartPan = 0;
  let pointerMoved = false;

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
    layout = layoutViewerModel(model, {
      width: cssWidth,
      height: cssHeight,
      zoom,
      panX,
    });
    const ratio = Math.max(1, globalThis.devicePixelRatio ?? 1);
    context.setTransform(ratio, 0, 0, ratio, 0, 0);
    renderViewerCanvas(context, layout, {
      ...(options.background === undefined
        ? {}
        : { background: options.background }),
      ...(hoveredNodeId === undefined ? {} : { hoveredNodeId }),
      ...(selectedNodeId === undefined ? {} : { selectedNodeId }),
      loading,
    });
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
          currentRegion = startedRegion;
          builder = new ViewerModelBuilder(region, budgets);
          model = builder.snapshot();
          trace = undefined;
          selectedNodeId = undefined;
          hoveredNodeId = undefined;
          zoom = 1;
          panX = 0;
          loading = true;
          detailElement.textContent =
            "anonymous weighted traversals are local evidence, not named individuals";
          emit("regionchange", startedRegion);
          scheduleRender();
        },
        onTile: (tile) => {
          builder?.addTile(tile);
          model = builder?.snapshot();
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
          loading = false;
          render();
        },
      });
    } catch (cause) {
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
  };
  const onPointerDown = (event: PointerEvent): void => {
    pointerId = event.pointerId;
    pointerStartX = event.clientX;
    pointerStartPan = panX;
    pointerMoved = false;
    canvas.setPointerCapture?.(event.pointerId);
    canvas.style.cursor = "grabbing";
  };
  const onPointerMove = (event: PointerEvent): void => {
    const [x, y] = pointerPosition(event);
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
    if (pointerId !== event.pointerId) return;
    const [x, y] = pointerPosition(event);
    if (!pointerMoved && layout !== undefined) {
      const selected = hitTestNode(layout, x, y);
      selectedNodeId = selected?.id;
      if (selected !== undefined)
        detailElement.textContent = nodeDescription(selected);
    }
    canvas.releasePointerCapture?.(event.pointerId);
    pointerId = undefined;
    canvas.style.cursor = "grab";
    scheduleRender();
  };
  const onPointerLeave = (): void => {
    if (pointerId !== undefined) return;
    hoveredNodeId = undefined;
    scheduleRender();
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
    getRegion: () => currentRegion,
    getSnapshot: (): ViewerSnapshot => ({
      ...(currentRegion === undefined ? {} : { region: currentRegion }),
      counts: model?.counts ?? emptyViewerCounts(),
      loading,
      zoom,
      panX,
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
    },
    panBy: (pixels) => {
      if (!Number.isFinite(pixels)) {
        throw new RangeError("viewer pan distance must be finite");
      }
      panX += pixels;
      scheduleRender();
    },
    resetView: () => {
      zoom = 1;
      panX = 0;
      scheduleRender();
    },
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

  if (options.initialRegion !== undefined) {
    void viewer.setRegion(options.initialRegion).catch(() => undefined);
  }
  return viewer;
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
