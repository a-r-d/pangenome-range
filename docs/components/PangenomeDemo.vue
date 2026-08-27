<script setup lang="ts">
// biome-ignore-all lint/correctness/noUnusedVariables: Vue template bindings are consumed outside the script AST.

import {
  type ArchiveInfo,
  type FeatureQueryTrace,
  type LocusHit,
  type OverviewBin,
  openPangenome,
  type PangenomeArchive,
  type QueryTrace,
  type ReferenceDescriptor,
  type RegionPlan,
  type RegionQuery,
} from "pangenome-range/reader";
import {
  chooseViewerLod,
  createPangenomeViewer,
  DEFAULT_COMPLEXITY_BUDGETS,
  formatGenomicCoordinate,
  type PangenomeViewer,
  parseGenomicCommand,
  recommendedSummaryBins,
  type ViewerDisplayMode,
  type ViewerLayerState,
  type ViewerLodDecision,
  type ViewerPerformanceSnapshot,
  type ViewerProgress,
} from "pangenome-range/viewer";
import { withBase } from "vitepress";
import {
  computed,
  nextTick,
  onBeforeUnmount,
  onMounted,
  ref,
  shallowRef,
  watch,
} from "vue";
import DetailedGraphStage from "./explorer/DetailedGraphStage.vue";
import EvidenceSheet from "./explorer/EvidenceSheet.vue";
import ExplorerTopbar from "./explorer/ExplorerTopbar.vue";
import InspectorDrawer from "./explorer/InspectorDrawer.vue";
import LoadingCard from "./explorer/LoadingCard.vue";
import OverviewStage from "./explorer/OverviewStage.vue";
import SearchPalette from "./explorer/SearchPalette.vue";
import ShowcaseStage from "./explorer/ShowcaseStage.vue";
import ToolRail from "./explorer/ToolRail.vue";
import type {
  ArchiveChoice,
  ExplorerPhase,
  ExplorerScreen,
  InspectorSelection,
  SearchState,
  SummaryMetric,
  SummaryScale,
} from "./explorer/types";

const configuredArchiveUrl =
  (
    import.meta.env.VITE_PANGENOME_RANGE_DEMO_ARCHIVE_URL as string | undefined
  )?.trim() ?? "";
const populationArchiveUrl =
  (
    import.meta.env.VITE_PANGENOME_RANGE_DEMO_1000G_ARCHIVE_URL as
      | string
      | undefined
  )?.trim() ?? "";
const configuredDefaultLocus =
  (
    import.meta.env.VITE_PANGENOME_RANGE_DEMO_DEFAULT_LOCUS as
      | string
      | undefined
  )?.trim() || "HLA-B";
const configuredDefaultSample =
  (
    import.meta.env.VITE_PANGENOME_RANGE_DEMO_DEFAULT_SAMPLE as
      | string
      | undefined
  )?.trim() || "GRCh38";
const configuredDefaultPadding = Math.max(
  0,
  Number.parseInt(
    (import.meta.env.VITE_PANGENOME_RANGE_DEMO_DEFAULT_PADDING as
      | string
      | undefined) ?? "2000",
    10,
  ) || 2_000,
);
const RECENT_URLS_KEY = "pangenome-range:recent-urls:v1";
const RECENT_SEARCHES_KEY = "pangenome-range:recent-searches:v1";
const LAYERS_KEY = "pangenome-range:layers:v1";
const THEME_KEY = "pangenome-range:theme:v2";
const viewerHost = ref<HTMLElement>();
const overviewStage = ref<{ getCanvas: () => HTMLCanvasElement | undefined }>();
const commandBar = ref<{ focus: () => void }>();
const localFileInput = ref<HTMLInputElement>();
const archiveChoice = ref<ArchiveChoice>(
  configuredArchiveUrl.length > 0
    ? "configured"
    : populationArchiveUrl.length > 0
      ? "population"
      : "fixture",
);
const customUrl = ref("");
const localFile = shallowRef<File>();
const recentUrls = ref<string[]>([]);
const recentSearches = ref<string[]>([]);
const archive = shallowRef<PangenomeArchive>();
const archiveInfo = shallowRef<ArchiveInfo>();
const viewer = shallowRef<PangenomeViewer>();
const references = ref<readonly ReferenceDescriptor[]>([]);
const phase = ref<ExplorerPhase>("idle");
const screen = ref<ExplorerScreen>("showcase");
const statusMessage = ref("Preparing the explorer…");
const errorMessage = ref("");
const activeArchiveLabel = ref("Not opened");
const activeSourceKey = ref("");
const command = ref("");
const suggestions = ref<readonly LocusHit[]>([]);
const searchState = ref<SearchState>("ready");
const searchTrace = shallowRef<FeatureQueryTrace>();
const searchTruncated = ref(false);
const searchMessage = ref("");
const activeSuggestion = ref(0);
const selectedLocus = shallowRef<LocusHit>();
const activeRegion = shallowRef<RegionQuery>();
const visualRegion = shallowRef<RegionQuery>();
const requestedRegion = shallowRef<RegionQuery>();
const loadedRegion = shallowRef<RegionQuery>();
const prefetchedRegion = shallowRef<RegionQuery>();
const summaryBins = shallowRef<readonly OverviewBin[]>([]);
const regionPlan = shallowRef<RegionPlan>();
const summaryTrace = shallowRef<FeatureQueryTrace>();
const queryTrace = shallowRef<QueryTrace>();
const progress = shallowRef<ViewerProgress>();
const lodDecision = shallowRef<ViewerLodDecision>();
const forceDetail = ref(false);
const summaryMetric = ref<SummaryMetric>("nodeRecords");
const summaryScale = ref<SummaryScale>("normalized");
const inspector = shallowRef<InspectorSelection>({ kind: "archive" });
const evidenceOpen = ref(false);
const detailRequested = ref(false);
const toolPanel = ref<"navigate" | "layers" | "tracks" | "detail" | null>(null);
const commandPaletteOpen = ref(false);
const sourceOpen = ref(false);
const shortcutsOpen = ref(false);
const technicalMode = ref(false);
const darkMode = ref(false);
const shareMessage = ref("");
const loadingContextRegion = shallowRef<RegionQuery>();
const loadingContextBins = shallowRef<readonly OverviewBin[]>();
const loadingContextPlan = shallowRef<RegionPlan>();
const summaryPaintMs = ref<number>();
const openMs = ref<number>();
const queryWallMs = ref<number>();
const viewerPerformance = shallowRef<ViewerPerformanceSnapshot>();
const layers = ref<ViewerLayerState>({
  reference: true,
  topology: true,
  traversals: true,
  tileBoundaries: true,
  sequenceLabels: false,
});
let sourceOperation = 0;
let regionOperation = 0;
let sourceController: AbortController | undefined;
let regionController: AbortController | undefined;
let searchController: AbortController | undefined;
let searchTimer: ReturnType<typeof setTimeout> | undefined;
let viewportTimer: ReturnType<typeof setTimeout> | undefined;
let summaryResizeObserver: ResizeObserver | undefined;
let viewerUnsubscribers: (() => void)[] = [];
let pointerId: number | undefined;
const summaryPointers = new Map<number, { x: number; y: number }>();
let pointerStartX = 0;
let pointerStartRegion: RegionQuery | undefined;
let summaryPinchDistance = 0;
let summaryPinchAnchor = 0;

const currentReference = computed(() => {
  const region = visualRegion.value ?? activeRegion.value;
  return region === undefined
    ? undefined
    : mergedReference(region.sample, region.contig);
});
const displayMode = computed<ViewerDisplayMode>(
  () => lodDecision.value?.mode ?? "overview",
);
const detailVisible = computed(
  () =>
    screen.value === "showcase" ||
    forceDetail.value ||
    archiveInfo.value?.summaries === undefined ||
    (detailRequested.value && lodDecision.value?.automaticDetail === true),
);
const loadingVisible = computed(
  () =>
    phase.value === "opening" ||
    (loadedRegion.value === undefined &&
      ["summary", "graph"].includes(phase.value)) ||
    (screen.value === "explorer" &&
      detailRequested.value &&
      ["summary", "graph"].includes(phase.value)),
);
const graphSurfaceMode = computed<"hidden" | "preview" | "detail">(() => {
  if (screen.value === "showcase") return "preview";
  if (detailVisible.value && phase.value === "ready") return "detail";
  return "hidden";
});
const overviewDisplayRegion = computed(
  () => loadingContextRegion.value ?? visualRegion.value ?? activeRegion.value,
);
const overviewDisplayBins = computed(
  () => loadingContextBins.value ?? summaryBins.value,
);
const overviewDisplayPlan = computed(
  () => loadingContextPlan.value ?? regionPlan.value,
);
const recommendedDetailRegion = computed<RegionQuery | undefined>(() => {
  const locus = selectedLocus.value;
  if (locus !== undefined) return paddedLocus(locus);
  const plan = regionPlan.value;
  const region = visualRegion.value ?? activeRegion.value;
  if (plan === undefined || region === undefined || plan.ranges.length === 0)
    return undefined;
  const candidate = [...plan.ranges].sort((left, right) =>
    left.decodedBytes === right.decodedBytes
      ? left.coreStart - right.coreStart
      : left.decodedBytes > right.decodedBytes
        ? -1
        : 1,
  )[0];
  if (candidate === undefined) return undefined;
  const center = (candidate.coreStart + candidate.coreEnd) / 2;
  const span = Math.min(12_000, region.end - region.start);
  return clampRegion({
    ...region,
    start: Math.round(center - span / 2),
    end: Math.round(center + span / 2),
  });
});
const canonicalCoordinate = computed(() => {
  const region = visualRegion.value ?? activeRegion.value;
  return region === undefined
    ? "—"
    : formatGenomicCoordinate(
        region.sample,
        region.contig,
        region.start,
        region.end,
      );
});
const archiveTitle = computed(
  () => archiveInfo.value?.provenance?.datasetTitle ?? activeArchiveLabel.value,
);
const metricLabel = computed(
  () =>
    ({
      coveredBases: "covered reference bases",
      tileCount: "regional tile count",
      encodedBytes: "encoded regional bytes",
      decodedBytes: "decoded regional bytes",
      nodeRecords: "node-record count",
      edgeRecords: "edge-record count",
      gbwtRecords: "GBWT-record count",
      occurrences: "occurrence count",
    })[summaryMetric.value],
);
const namedCommandActive = computed(() => {
  try {
    return (
      parseGenomicCommand(
        command.value,
        references.value,
        activeRegion.value?.sample,
      ).kind === "locus"
    );
  } catch {
    return command.value.trim().length > 0;
  }
});

function getSummaryCanvas(): HTMLCanvasElement | undefined {
  return overviewStage.value?.getCanvas();
}

function summaryViewportWidth(): number {
  return Math.max(320, getSummaryCanvas()?.clientWidth || 900);
}

onMounted(async () => {
  document.body.classList.add("pangenome-explorer-active");
  restorePreferences();
  restoreUrlState();
  window.addEventListener("popstate", onPopState);
  window.addEventListener("keydown", onGlobalKeyDown, { capture: true });
  await nextTick();
  const canvas = getSummaryCanvas();
  if (canvas !== undefined) {
    summaryResizeObserver = new ResizeObserver(drawSummary);
    summaryResizeObserver.observe(canvas);
  }
  await loadSource().catch(() => undefined);
});

onBeforeUnmount(() => {
  sourceOperation += 1;
  regionOperation += 1;
  sourceController?.abort();
  regionController?.abort();
  searchController?.abort();
  if (searchTimer !== undefined) clearTimeout(searchTimer);
  if (viewportTimer !== undefined) clearTimeout(viewportTimer);
  summaryResizeObserver?.disconnect();
  detachViewer();
  void archive.value?.close();
  window.removeEventListener("popstate", onPopState);
  window.removeEventListener("keydown", onGlobalKeyDown, { capture: true });
  document.body.classList.remove("pangenome-explorer-active");
});

watch(command, scheduleSuggestions);
watch([summaryMetric, summaryScale], drawSummary);
watch(
  layers,
  (value) => {
    viewer.value?.setLayers(value);
    localStorage.setItem(LAYERS_KEY, JSON.stringify(value));
  },
  { deep: true },
);
watch(forceDetail, () => {
  const region = activeRegion.value;
  if (region !== undefined) void loadRegion(region);
});

async function loadSource(): Promise<void> {
  const operation = ++sourceOperation;
  const previousArchive = archive.value;
  errorMessage.value = "";
  sourceController?.abort();
  sourceController = new AbortController();
  regionController?.abort();
  searchController?.abort();
  const selected = selectedSource();
  sourceOpen.value = false;
  phase.value = "opening";
  statusMessage.value = `Opening ${selected.label}`;
  const started = performance.now();
  try {
    const opened = await openPangenome({
      source: selected.source,
      signal: sourceController.signal,
      httpUseHead: false,
    });
    if (operation !== sourceOperation) {
      await opened.close();
      return;
    }
    openMs.value = performance.now() - started;
    const openedInfo = await opened.info({ signal: sourceController.signal });
    if (operation !== sourceOperation) {
      await opened.close();
      return;
    }
    detachViewer();
    await previousArchive?.close();
    archive.value = opened;
    archiveInfo.value = openedInfo;
    selectedLocus.value = undefined;
    inspector.value = { kind: "archive" };
    summaryBins.value = [];
    regionPlan.value = undefined;
    activeSourceKey.value = selected.key;
    activeArchiveLabel.value = selected.label;
    references.value = opened.references();
    searchState.value =
      archiveInfo.value.namedLoci.state === "absent"
        ? "index-absent"
        : archiveInfo.value.namedLoci.state === "present-empty"
          ? "index-empty"
          : "ready";
    if (archiveChoice.value === "custom") rememberRemoteUrl(customUrl.value);
    await nextTick();
    createViewer(opened);
    const restored = regionFromUrl();
    const initial = restored ?? (await defaultRegion(opened));
    screen.value = restored === undefined ? "showcase" : "explorer";
    detailRequested.value =
      restored === undefined ||
      new URLSearchParams(window.location.search).get("view") === "detail";
    command.value =
      selectedLocus.value?.displayName ??
      formatGenomicCoordinate(
        initial.sample,
        initial.contig,
        initial.start,
        initial.end,
      );
    await navigateTo(initial, "none");
  } catch (cause) {
    if (operation !== sourceOperation || isAbort(cause)) return;
    fail(cause, "The archive could not be opened.");
  }
}

function createViewer(opened: PangenomeArchive): void {
  if (viewerHost.value === undefined)
    throw new Error("Viewer host did not mount.");
  const next = createPangenomeViewer(viewerHost.value, {
    archive: opened,
    maxRenderedNodes: 160,
    maxRenderedEdges: 260,
    maxHaplotypeLanes: 6,
    showRequestTrace: false,
    initialLayers: layers.value,
    initialTheme: "dark",
  });
  const viewerRoot = viewerHost.value.querySelector<HTMLElement>(
    '[data-pangenome-viewer="true"]',
  );
  const viewerCanvas = viewerHost.value.querySelector<HTMLCanvasElement>(
    '[data-viewer-canvas="true"]',
  );
  if (viewerRoot !== null) {
    Object.assign(viewerRoot.style, {
      display: "flex",
      height: "100%",
      minHeight: "0",
      flexDirection: "column",
      border: "0",
      borderRadius: "18px",
      background: "transparent",
      boxShadow: "none",
    });
    for (const element of viewerRoot.querySelectorAll<HTMLElement>(
      ":scope > div:not([role=status])",
    )) {
      element.style.borderColor = "#244160";
      element.style.color = "#91a7c2";
    }
  }
  if (viewerCanvas !== null) {
    Object.assign(viewerCanvas.style, {
      minHeight: "280px",
      flex: "1",
      height: "calc(100% - 72px)",
    });
  }
  viewer.value = next;
  viewerUnsubscribers = [
    next.on("progress", (detail) => {
      progress.value = detail;
      statusMessage.value = `Decoded ${detail.counts.tiles} graph tile${detail.counts.tiles === 1 ? "" : "s"}`;
    }),
    next.on("querytrace", (trace) => {
      queryTrace.value = trace;
      viewerPerformance.value = next.getPerformanceSnapshot();
    }),
    next.on("selectionchange", (detail) => {
      inspector.value = detail ?? { kind: "archive" };
    }),
    next.on("viewportchange", ({ visualRegion: region }) => {
      visualRegion.value = clampRegion(region);
      scheduleViewportSettlement();
    }),
    next.on("error", ({ error }) => {
      if (!isAbort(error)) fail(error, "Detailed graph loading failed.");
    }),
  ];
}

async function defaultRegion(opened: PangenomeArchive): Promise<RegionQuery> {
  const first = references.value[0];
  if (first === undefined) throw new Error("Archive contains no references.");
  if (opened.capabilities().namedLoci) {
    try {
      const result = await opened.searchLoci({
        name: configuredDefaultLocus,
        mode: "exact",
        sample: configuredDefaultSample,
        limit: 1,
      });
      const hit = result.hits[0];
      if (hit !== undefined) {
        selectedLocus.value = hit;
        return paddedLocus(hit, configuredDefaultPadding);
      }
    } catch {
      // Coordinate exploration remains available if an optional index fails.
    }
  }
  const preferred =
    references.value.find(
      (reference) => reference.sample === configuredDefaultSample,
    ) ?? first;
  return {
    sample: preferred.sample,
    contig: preferred.contig,
    start: preferred.start,
    end: Math.min(preferred.end, preferred.start + 100_000),
    context: 100,
  };
}

async function navigateTo(
  region: RegionQuery,
  history: "push" | "replace" | "none" = "push",
): Promise<void> {
  const bounded = clampRegion(region);
  activeRegion.value = bounded;
  visualRegion.value = bounded;
  if (history !== "none") updateUrlState(history);
  await loadRegion(bounded);
}

async function loadRegion(region: RegionQuery): Promise<void> {
  const opened = archive.value;
  if (opened === undefined) return;
  const operation = ++regionOperation;
  regionController?.abort();
  regionController = new AbortController();
  const signal = regionController.signal;
  requestedRegion.value = region;
  errorMessage.value = "";
  summaryTrace.value = undefined;
  queryTrace.value = undefined;
  progress.value = undefined;
  regionPlan.value = undefined;
  queryWallMs.value = undefined;
  lodDecision.value = undefined;
  const started = performance.now();
  try {
    if (opened.capabilities().multiscaleSummaries) {
      phase.value = "summary";
      statusMessage.value = `Planning ranges and reading the regional overview for ${formatShortRegion(region)}`;
      const summaryStarted = performance.now();
      const [plan, result] = await Promise.all([
        opened.planRegion({ ...region, signal }),
        opened.summary({
          ...region,
          maxBins: recommendedSummaryBins(summaryViewportWidth()),
          signal,
          trace: true,
        }),
      ]);
      if (operation !== regionOperation) return;
      regionPlan.value = plan;
      summaryBins.value = result.bins;
      summaryTrace.value = result.trace;
      lodDecision.value = chooseViewerLod(
        result.bins,
        region,
        summaryViewportWidth(),
        DEFAULT_COMPLEXITY_BUDGETS,
        forceDetail.value,
        plan,
      );
      await nextTick();
      drawSummary();
      summaryPaintMs.value = performance.now() - summaryStarted;
    } else {
      summaryBins.value = [];
      regionPlan.value = await opened.planRegion({ ...region, signal });
      lodDecision.value = {
        mode: "detailed",
        automaticDetail: true,
        bpPerPixel: (region.end - region.start) / summaryViewportWidth(),
        estimates: zeroBudgets(),
        budgets: DEFAULT_COMPLEXITY_BUDGETS,
        limitingMetrics: [],
        usesPartialBinEstimates: false,
        reason: "Summary index absent; using bounded detailed graph fallback.",
      };
      await nextTick();
      drawSummary();
      summaryPaintMs.value = performance.now() - started;
    }
    if (operation !== regionOperation) return;
    if (detailVisible.value) {
      phase.value = "graph";
      statusMessage.value = `Streaming detailed graph for ${formatShortRegion(region)}`;
      viewer.value?.setDisplayMode(displayMode.value);
      await viewer.value?.setViewport({ ...region, signal });
      if (operation !== regionOperation) return;
      viewerPerformance.value = viewer.value?.getPerformanceSnapshot();
    }
    loadedRegion.value = region;
    prefetchedRegion.value = predictedAdjacentRegion(region);
    queryWallMs.value = performance.now() - started;
    phase.value = "ready";
    loadingContextRegion.value = undefined;
    loadingContextBins.value = undefined;
    loadingContextPlan.value = undefined;
    statusMessage.value = detailVisible.value
      ? "Graph ready. Pan or zoom to settle a new genomic query."
      : "Summary ready. Zoom in or load the detailed graph explicitly.";
  } catch (cause) {
    if (operation !== regionOperation || isAbort(cause)) return;
    fail(cause, "This genomic viewport could not be loaded.");
  }
}

function scheduleSuggestions(): void {
  if (searchTimer !== undefined) clearTimeout(searchTimer);
  searchController?.abort();
  suggestions.value = [];
  activeSuggestion.value = 0;
  searchTruncated.value = false;
  searchMessage.value = "";
  const opened = archive.value;
  if (opened === undefined || command.value.trim().length === 0) return;
  try {
    const parsed = parseGenomicCommand(
      command.value,
      references.value,
      activeRegion.value?.sample,
    );
    if (parsed.kind === "coordinate") {
      searchState.value = "ready";
      return;
    }
  } catch (cause) {
    searchMessage.value =
      cause instanceof Error ? cause.message : String(cause);
    return;
  }
  if (!opened.capabilities().namedLoci) {
    searchState.value = "index-absent";
    return;
  }
  if (archiveInfo.value?.namedLoci.state === "present-empty") {
    searchState.value = "index-empty";
    return;
  }
  searchTimer = setTimeout(() => void searchPrefix(), 180);
}

function onCommandKeyDown(event: KeyboardEvent): void {
  if (event.key === "Escape") {
    event.preventDefault();
    suggestions.value = [];
    commandPaletteOpen.value = false;
    return;
  }
  if (suggestions.value.length === 0) {
    return;
  }
  if (event.key === "ArrowDown") {
    event.preventDefault();
    activeSuggestion.value =
      (activeSuggestion.value + 1) % suggestions.value.length;
  } else if (event.key === "ArrowUp") {
    event.preventDefault();
    activeSuggestion.value =
      (activeSuggestion.value - 1 + suggestions.value.length) %
      suggestions.value.length;
  } else if (event.key === "Enter") {
    event.preventDefault();
    const hit = suggestions.value[activeSuggestion.value];
    if (hit !== undefined) void selectLocus(hit);
  }
}

async function searchPrefix(): Promise<void> {
  const opened = archive.value;
  if (opened === undefined) return;
  searchController?.abort();
  searchController = new AbortController();
  const input = command.value.trim();
  searchState.value = "searching";
  try {
    const result = await opened.searchLoci({
      name: input,
      mode: "prefix",
      limit: 12,
      signal: searchController.signal,
      trace: true,
    });
    suggestions.value = result.hits;
    searchTrace.value = result.trace;
    searchTruncated.value = result.truncated;
    searchState.value = result.truncated
      ? "truncated"
      : result.hits.length === 0
        ? "no-matches"
        : "results";
  } catch (cause) {
    if (isAbort(cause)) return;
    searchState.value = "failed";
    searchMessage.value = actionableError(cause);
  }
}

async function submitCommand(): Promise<void> {
  try {
    searchController?.abort();
    const parsed = parseGenomicCommand(
      command.value,
      references.value,
      activeRegion.value?.sample,
    );
    rememberSearch(command.value.trim());
    if (parsed.kind === "coordinate") {
      detailRequested.value = false;
      selectedLocus.value = undefined;
      suggestions.value = [];
      commandPaletteOpen.value = false;
      await navigateTo({
        sample: parsed.reference.sample,
        contig: parsed.reference.contig,
        start: parsed.start,
        end: parsed.end,
        context: 100,
      });
      return;
    }
    const opened = archive.value;
    if (opened === undefined) return;
    searchState.value = "searching";
    const exact = await opened.searchLoci({
      name: parsed.name,
      mode: "exact",
      limit: 20,
      trace: true,
    });
    searchTrace.value = exact.trace;
    if (exact.hits.length === 1) {
      await selectLocus(exact.hits[0] as LocusHit);
    } else {
      suggestions.value = exact.hits;
      searchTruncated.value = exact.truncated;
      searchState.value = exact.hits.length === 0 ? "no-matches" : "results";
    }
  } catch (cause) {
    searchState.value = "failed";
    searchMessage.value = actionableError(cause);
  }
}

async function selectLocus(
  hit: LocusHit,
  intent: "detail" | "overview" = locusIntent(hit),
): Promise<void> {
  screen.value = "explorer";
  detailRequested.value = intent === "detail";
  selectedLocus.value = hit;
  inspector.value = {
    kind: "locus",
    hit,
    matchedAlias: hit.matchedName !== hit.displayName,
  };
  suggestions.value = [];
  commandPaletteOpen.value = false;
  command.value = hit.displayName;
  rememberSearch(hit.displayName);
  if (intent === "detail") captureLoadingContext();
  await navigateTo(
    intent === "detail" ? paddedLocus(hit) : overviewRegionForLocus(hit),
  );
}

function locusIntent(hit: LocusHit): "detail" | "overview" {
  return hit.reference.end - hit.reference.start <= 120_000
    ? "detail"
    : "overview";
}

function paddedLocus(hit: LocusHit, configuredPadding?: number): RegionQuery {
  const span = hit.reference.end - hit.reference.start;
  const padding = configuredPadding ?? Math.max(1_000, Math.round(span * 0.15));
  return clampRegion({
    sample: hit.reference.sample,
    contig: hit.reference.contig,
    start: Math.max(0, hit.reference.start - padding),
    end: hit.reference.end + padding,
    context: 100,
  });
}

function overviewRegionForLocus(hit: LocusHit): RegionQuery {
  const reference = mergedReference(hit.reference.sample, hit.reference.contig);
  const baseSpan = archiveInfo.value?.summaries?.baseBinSpan ?? 1_048_576;
  const locusSpan = hit.reference.end - hit.reference.start;
  const span = Math.max(baseSpan * 10, locusSpan * 8, 2_000_000);
  const center = (hit.reference.start + hit.reference.end) / 2;
  return clampRegion({
    sample: hit.reference.sample,
    contig: hit.reference.contig,
    start: Math.max(reference?.start ?? 0, Math.round(center - span / 2)),
    end: Math.min(
      reference?.end ?? Number.MAX_SAFE_INTEGER,
      Math.round(center + span / 2),
    ),
    context: 100,
  });
}

function captureLoadingContext(): void {
  if (screen.value !== "explorer" || visualRegion.value === undefined) return;
  loadingContextRegion.value = visualRegion.value;
  loadingContextBins.value = summaryBins.value;
  loadingContextPlan.value = regionPlan.value;
}

function openShowcase(): void {
  screen.value = "showcase";
  commandPaletteOpen.value = false;
  const params = new URLSearchParams();
  if (archiveChoice.value !== "configured")
    params.set("archive", archiveChoice.value);
  if (archiveChoice.value === "custom")
    params.set("url", customUrl.value.trim());
  const query = params.size === 0 ? "" : `?${params}`;
  window.history.pushState(null, "", `${window.location.pathname}${query}`);
}

function openShowcaseDetail(): void {
  screen.value = "explorer";
  detailRequested.value = true;
  const locus = selectedLocus.value;
  const loaded = loadedRegion.value;
  if (
    locus !== undefined &&
    (loaded === undefined ||
      loaded.start > locus.reference.start ||
      loaded.end < locus.reference.end ||
      queryTrace.value === undefined)
  ) {
    captureLoadingContext();
    void navigateTo(paddedLocus(locus));
    return;
  }
  updateUrlState("push");
}

function openShowcaseOverview(): void {
  const locus = selectedLocus.value;
  if (locus === undefined) return;
  screen.value = "explorer";
  detailRequested.value = false;
  void navigateTo(overviewRegionForLocus(locus));
}

function useRecentSearch(value: string): void {
  command.value = value;
  commandPaletteOpen.value = true;
  void submitCommand();
}

function openRecommendedDetail(): void {
  const region = recommendedDetailRegion.value;
  if (region === undefined) return;
  captureLoadingContext();
  screen.value = "explorer";
  detailRequested.value = true;
  forceDetail.value = false;
  void navigateTo(region);
}

function cancelCurrentLoad(): void {
  sourceController?.abort();
  regionController?.abort();
  statusMessage.value =
    "Loading cancelled. The previous view remains available.";
  if (loadingContextRegion.value !== undefined) {
    activeRegion.value = loadingContextRegion.value;
    visualRegion.value = loadingContextRegion.value;
    summaryBins.value = loadingContextBins.value ?? summaryBins.value;
    regionPlan.value = loadingContextPlan.value;
    detailRequested.value = false;
  }
  loadingContextRegion.value = undefined;
  loadingContextBins.value = undefined;
  loadingContextPlan.value = undefined;
  if (loadedRegion.value !== undefined) phase.value = "ready";
}

function selectedSource(): {
  source: string | Blob;
  key: string;
  label: string;
} {
  if (archiveChoice.value === "local") {
    const file = localFile.value;
    if (file === undefined) throw new Error("Choose a local .pngr file first.");
    return {
      source: file,
      key: `file:${file.name}:${file.size}:${file.lastModified}`,
      label: `${file.name} (local file)`,
    };
  }
  if (archiveChoice.value === "fixture") {
    const url = new URL(
      withBase("/fixtures/format-v1.pngr"),
      window.location.href,
    ).href;
    return {
      source: url,
      key: `url:${url}`,
      label: "Bundled deterministic fixture",
    };
  }
  if (archiveChoice.value === "configured") {
    if (configuredArchiveUrl.length === 0)
      throw new Error("No external archive is configured for this build.");
    const url = new URL(configuredArchiveUrl, window.location.href).href;
    return {
      source: url,
      key: `url:${url}`,
      label: "HPRC v2.1 + GENCODE v50",
    };
  }
  if (archiveChoice.value === "population") {
    if (populationArchiveUrl.length === 0)
      throw new Error("No 1000 Genomes archive is configured for this build.");
    const url = new URL(populationArchiveUrl, window.location.href).href;
    return {
      source: url,
      key: `url:${url}`,
      label: "1000 Genomes hs38d1 — NA19239 haplotype 0",
    };
  }
  if (customUrl.value.trim().length === 0)
    throw new Error("Enter an archive URL.");
  const url = new URL(customUrl.value.trim(), window.location.href).href;
  return { source: url, key: `url:${url}`, label: "Custom remote archive" };
}

function onSourceChange(): void {
  errorMessage.value = "";
  if (archiveChoice.value === "local" && localFile.value === undefined) {
    localFileInput.value?.click();
    return;
  }
  if (archiveChoice.value === "custom") {
    sourceOpen.value = true;
    return;
  }
  void loadSource();
}

function onLocalFile(event: Event): void {
  const file = (event.currentTarget as HTMLInputElement).files?.[0];
  if (file === undefined) return;
  localFile.value = file;
  archiveChoice.value = "local";
  void loadSource();
}

function useRecentUrl(url: string): void {
  customUrl.value = url;
  archiveChoice.value = "custom";
  void loadSource();
}

function scheduleViewportSettlement(): void {
  drawSummary();
  if (viewportTimer !== undefined) clearTimeout(viewportTimer);
  viewportTimer = setTimeout(() => {
    const region = visualRegion.value;
    if (region === undefined) return;
    activeRegion.value = region;
    updateUrlState("replace");
    void loadRegion(region);
  }, 180);
}

function panRegion(fraction: number): void {
  const region = visualRegion.value;
  if (region === undefined) return;
  const delta = Math.round((region.end - region.start) * fraction);
  visualRegion.value = clampRegion({
    ...region,
    start: region.start + delta,
    end: region.end + delta,
  });
  scheduleViewportSettlement();
}

function zoomRegion(factor: number, focus = 0.5): void {
  const region = visualRegion.value;
  if (region === undefined) return;
  const span = region.end - region.start;
  const nextSpan = Math.max(1, Math.round(span * factor));
  const anchor = region.start + span * focus;
  const start = Math.round(anchor - nextSpan * focus);
  visualRegion.value = clampRegion({
    ...region,
    start,
    end: start + nextSpan,
  });
  scheduleViewportSettlement();
}

function fitReference(): void {
  const reference = currentReference.value;
  if (reference !== undefined) void navigateTo({ ...reference, context: 100 });
}

function fitLocus(): void {
  if (selectedLocus.value !== undefined)
    void navigateTo(paddedLocus(selectedLocus.value));
}

function onSummaryWheel(event: WheelEvent): void {
  event.preventDefault();
  const canvas = getSummaryCanvas();
  if (canvas === undefined) return;
  const rect = canvas.getBoundingClientRect();
  const focus = Math.min(
    1,
    Math.max(0, (event.clientX - rect.left) / rect.width),
  );
  zoomRegion(Math.exp(event.deltaY * 0.0015), focus);
}

function onSummaryPointerDown(event: PointerEvent): void {
  summaryPointers.set(event.pointerId, { x: event.clientX, y: event.clientY });
  pointerId = event.pointerId;
  pointerStartX = event.clientX;
  pointerStartRegion = visualRegion.value;
  getSummaryCanvas()?.setPointerCapture?.(event.pointerId);
  if (summaryPointers.size === 2 && pointerStartRegion !== undefined) {
    const [first, second] = [...summaryPointers.values()];
    const canvas = getSummaryCanvas();
    if (first !== undefined && second !== undefined && canvas !== undefined) {
      summaryPinchDistance = Math.max(
        1,
        Math.hypot(first.x - second.x, first.y - second.y),
      );
      const rect = canvas.getBoundingClientRect();
      const focus = ((first.x + second.x) / 2 - rect.left) / rect.width;
      summaryPinchAnchor =
        pointerStartRegion.start +
        (pointerStartRegion.end - pointerStartRegion.start) * focus;
    }
  }
}

function onSummaryPointerMove(event: PointerEvent): void {
  if (summaryPointers.has(event.pointerId)) {
    summaryPointers.set(event.pointerId, {
      x: event.clientX,
      y: event.clientY,
    });
  }
  if (pointerStartRegion === undefined) return;
  const canvas = getSummaryCanvas();
  if (canvas === undefined) return;
  if (summaryPointers.size === 2 && summaryPinchDistance > 0) {
    const [first, second] = [...summaryPointers.values()];
    if (first === undefined || second === undefined) return;
    const distance = Math.max(
      1,
      Math.hypot(first.x - second.x, first.y - second.y),
    );
    const rect = canvas.getBoundingClientRect();
    const focus = Math.min(
      1,
      Math.max(0, ((first.x + second.x) / 2 - rect.left) / rect.width),
    );
    const startSpan = pointerStartRegion.end - pointerStartRegion.start;
    const span = Math.max(
      1,
      Math.round(startSpan * (summaryPinchDistance / distance)),
    );
    const start = Math.round(summaryPinchAnchor - span * focus);
    visualRegion.value = clampRegion({
      ...pointerStartRegion,
      start,
      end: start + span,
    });
    drawSummary();
    return;
  }
  if (pointerId !== event.pointerId) return;
  const span = pointerStartRegion.end - pointerStartRegion.start;
  const delta = Math.round(
    ((pointerStartX - event.clientX) / Math.max(1, canvas.clientWidth)) * span,
  );
  visualRegion.value = clampRegion({
    ...pointerStartRegion,
    start: pointerStartRegion.start + delta,
    end: pointerStartRegion.end + delta,
  });
  drawSummary();
}

function onSummaryPointerUp(event: PointerEvent): void {
  if (!summaryPointers.has(event.pointerId)) return;
  const wasPinching = summaryPointers.size > 1 || summaryPinchDistance > 0;
  const moved = Math.abs(event.clientX - pointerStartX) > 3;
  getSummaryCanvas()?.releasePointerCapture?.(event.pointerId);
  summaryPointers.delete(event.pointerId);
  const remaining = summaryPointers.entries().next().value as
    | [number, { x: number; y: number }]
    | undefined;
  pointerId = remaining?.[0];
  if (remaining !== undefined) {
    pointerStartX = remaining[1].x;
    pointerStartRegion = visualRegion.value;
  } else {
    pointerStartRegion = undefined;
    summaryPinchDistance = 0;
  }
  if (summaryPointers.size === 0) {
    if (moved || wasPinching) scheduleViewportSettlement();
    else selectSummaryAt(event.clientX);
  }
}

function selectSummaryAt(clientX: number): void {
  const canvas = getSummaryCanvas();
  const region = visualRegion.value;
  if (canvas === undefined || region === undefined) return;
  const rect = canvas.getBoundingClientRect();
  const coordinate =
    region.start +
    ((clientX - rect.left) / rect.width) * (region.end - region.start);
  const bin = summaryBins.value.find(
    (candidate) =>
      candidate.reference.start <= coordinate &&
      candidate.reference.end > coordinate,
  );
  if (bin !== undefined) inspector.value = { kind: "summary", bin };
}

function drawSummary(): void {
  const canvas = getSummaryCanvas();
  const region = overviewDisplayRegion.value;
  if (canvas === undefined || region === undefined) return;
  const width = Math.max(320, canvas.clientWidth || 900);
  const height = Math.max(120, canvas.clientHeight || 260);
  const ratio = Math.max(1, window.devicePixelRatio || 1);
  if (canvas.width !== Math.round(width * ratio))
    canvas.width = Math.round(width * ratio);
  if (canvas.height !== Math.round(height * ratio))
    canvas.height = Math.round(height * ratio);
  const context = canvas.getContext("2d");
  if (context === null) return;
  context.setTransform(ratio, 0, 0, ratio, 0, 0);
  const dark = darkMode.value;
  const colors = dark
    ? {
        background: "#0b1220",
        grid: "#243247",
        text: "#9aaabd",
        topology: "#2dd4bf",
        topologyFill: "rgba(45, 212, 191, .18)",
        traversal: "#9b8cff",
        transfer: "#f28a62",
        highlight: "#5eead4",
      }
    : {
        background: "#f8fafc",
        grid: "#e5eaf0",
        text: "#647386",
        topology: "#0ea89d",
        topologyFill: "rgba(14, 168, 157, .16)",
        traversal: "#7c73f4",
        transfer: "#ee7650",
        highlight: "#00a99d",
      };
  context.fillStyle = colors.background;
  context.fillRect(0, 0, width, height);
  const plot = {
    left: 20,
    top: 18,
    width: width - 40,
    height: height - 72,
  };
  for (let index = 0; index <= 6; index += 1) {
    const x = plot.left + (plot.width * index) / 6;
    context.strokeStyle = colors.grid;
    context.lineWidth = 1;
    context.beginPath();
    context.moveTo(x, plot.top);
    context.lineTo(x, plot.top + plot.height);
    context.stroke();
  }
  context.strokeStyle = colors.grid;
  context.beginPath();
  context.moveTo(plot.left, plot.top + plot.height);
  context.lineTo(plot.left + plot.width, plot.top + plot.height);
  context.stroke();
  const visible = overviewDisplayBins.value.filter(
    (bin) =>
      bin.reference.start < region.end && bin.reference.end > region.start,
  );
  const span = Math.max(1, region.end - region.start);
  const xFor = (coordinate: number) =>
    plot.left + ((coordinate - region.start) / span) * plot.width;
  const scaledSeries = (values: number[]) => {
    const transformed =
      summaryScale.value === "linear"
        ? values
        : values.map((value) => Math.log1p(value));
    if (summaryScale.value === "normalized") {
      const minimum = Math.min(...transformed);
      const maximum = Math.max(...transformed);
      const range = maximum - minimum;
      return range === 0
        ? transformed.map(() => 0.5)
        : transformed.map((value) => (value - minimum) / range);
    }
    const maximum = Math.max(1, ...transformed);
    return transformed.map((value) => value / maximum);
  };
  const topologyValues = scaledSeries(
    visible.map((bin) => Number(bin.nodeRecords + bin.edgeRecords)),
  );
  const traversalValues = scaledSeries(
    visible.map((bin) => Number(bin.occurrences)),
  );
  const topologyBase = plot.top + plot.height * 0.62;
  context.beginPath();
  context.moveTo(plot.left, topologyBase);
  for (let index = 0; index < visible.length; index += 1) {
    const bin = visible[index] as OverviewBin;
    const x = xFor(
      Math.max(
        region.start,
        Math.min(region.end, (bin.reference.start + bin.reference.end) / 2),
      ),
    );
    const score = topologyValues[index] ?? 0;
    const y =
      topologyBase -
      (summaryScale.value === "normalized" ? 0.28 + score * 0.72 : score) *
        plot.height *
        0.52;
    context.lineTo(x, y);
  }
  context.lineTo(plot.left + plot.width, topologyBase);
  context.closePath();
  context.fillStyle = colors.topologyFill;
  context.fill();
  context.strokeStyle = colors.topology;
  context.lineWidth = 2.5;
  context.stroke();

  context.beginPath();
  for (let index = 0; index < visible.length; index += 1) {
    const bin = visible[index] as OverviewBin;
    const x = xFor((bin.reference.start + bin.reference.end) / 2);
    const y =
      plot.top +
      plot.height * 0.82 -
      (summaryScale.value === "normalized"
        ? 0.18 + (traversalValues[index] ?? 0) * 0.82
        : (traversalValues[index] ?? 0)) *
        plot.height *
        0.16;
    if (index === 0) context.moveTo(x, y);
    else context.lineTo(x, y);
  }
  context.strokeStyle = colors.traversal;
  context.lineWidth = 2.2;
  context.stroke();

  const planned = overviewDisplayPlan.value?.ranges ?? [];
  const maximumTransfer = Math.max(
    1,
    ...planned.map((range) => Number(range.compressedBytes)),
  );
  for (const range of planned) {
    const x1 = xFor(Math.max(region.start, range.coreStart));
    const x2 = xFor(Math.min(region.end, range.coreEnd));
    const transferHeight =
      (Number(range.compressedBytes) / maximumTransfer) * 10 + 2;
    context.fillStyle = colors.transfer;
    context.fillRect(
      x1,
      plot.top + plot.height - transferHeight,
      Math.max(2, x2 - x1 - 1),
      transferHeight,
    );
  }
  const recommendation = recommendedDetailRegion.value;
  if (recommendation !== undefined) {
    const x1 = xFor(recommendation.start);
    const x2 = xFor(recommendation.end);
    context.strokeStyle = colors.highlight;
    context.lineWidth = 2;
    context.setLineDash([6, 4]);
    context.strokeRect(x1, plot.top + 2, Math.max(6, x2 - x1), plot.height - 4);
    context.setLineDash([]);
  }
  context.fillStyle = colors.text;
  context.font = "11px ui-monospace, SFMono-Regular, Menlo, monospace";
  context.textAlign = "left";
  context.fillText(formatCoordinate(region.start), plot.left, height - 10);
  context.textAlign = "right";
  context.fillText(
    formatCoordinate(region.end),
    plot.left + plot.width,
    height - 10,
  );
}

async function copyRegionLink(): Promise<void> {
  if (archiveChoice.value === "local") {
    shareMessage.value =
      "Local files stay local and cannot be embedded in a link.";
    return;
  }
  updateUrlState("replace");
  await copyText(window.location.href, "Region link copied.");
}

async function copyCoordinate(): Promise<void> {
  await copyText(canonicalCoordinate.value, "Canonical coordinate copied.");
}

async function copyNodeSequence(): Promise<void> {
  if (inspector.value.kind !== "node") return;
  await copyText(inspector.value.node.sequence, "Node sequence copied.");
}

async function copyText(value: string, success: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(value);
    shareMessage.value = success;
  } catch {
    shareMessage.value =
      "Clipboard unavailable; select and copy the visible value.";
  }
}

function toggleTheme(): void {
  darkMode.value = !darkMode.value;
  document.documentElement.classList.toggle("dark", darkMode.value);
  const theme = darkMode.value ? "dark" : "light";
  localStorage.setItem(THEME_KEY, theme);
  viewer.value?.setTheme("dark");
  drawSummary();
}

function onGlobalKeyDown(event: KeyboardEvent): void {
  const target = event.target as HTMLElement | null;
  const typing =
    target?.matches("input, textarea, select, [contenteditable=true]") === true;
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
    event.preventDefault();
    event.stopImmediatePropagation();
    commandPaletteOpen.value = true;
    commandBar.value?.focus();
  } else if (event.key === "/" && !typing) {
    event.preventDefault();
    commandPaletteOpen.value = true;
    commandBar.value?.focus();
  } else if (event.key === "?" && !typing) {
    shortcutsOpen.value = !shortcutsOpen.value;
  } else if (event.key === "Escape") {
    suggestions.value = [];
    commandPaletteOpen.value = false;
    toolPanel.value = null;
    sourceOpen.value = false;
    shortcutsOpen.value = false;
  }
}

function onPopState(): void {
  const region = regionFromUrl();
  if (region === undefined) {
    screen.value = "showcase";
    return;
  }
  screen.value = "explorer";
  detailRequested.value =
    new URLSearchParams(window.location.search).get("view") === "detail";
  void navigateTo(region, "none");
}

function updateUrlState(mode: "push" | "replace"): void {
  if (archiveChoice.value === "local") return;
  const region = activeRegion.value;
  if (region === undefined) return;
  const params = new URLSearchParams();
  params.set("archive", archiveChoice.value);
  if (archiveChoice.value === "custom")
    params.set("url", customUrl.value.trim());
  params.set("view", detailVisible.value ? "detail" : "overview");
  params.set("sample", region.sample);
  params.set("contig", region.contig);
  params.set("start", String(region.start));
  params.set("end", String(region.end));
  if (selectedLocus.value !== undefined)
    params.set("locus", selectedLocus.value.displayName);
  const url = `${window.location.pathname}?${params}`;
  if (mode === "push") window.history.pushState(null, "", url);
  else window.history.replaceState(null, "", url);
}

function restoreUrlState(): void {
  const params = new URLSearchParams(window.location.search);
  const choice = params.get("archive");
  if (choice === "fixture") archiveChoice.value = "fixture";
  else if (choice === "configured" && configuredArchiveUrl.length > 0)
    archiveChoice.value = "configured";
  else if (choice === "population" && populationArchiveUrl.length > 0)
    archiveChoice.value = "population";
  else if (choice === "custom" && params.has("url")) {
    archiveChoice.value = "custom";
    customUrl.value = params.get("url") ?? "";
  }
}

function regionFromUrl(): RegionQuery | undefined {
  const params = new URLSearchParams(window.location.search);
  const sample = params.get("sample");
  const contig = params.get("contig");
  const start = safeInteger(params.get("start"));
  const end = safeInteger(params.get("end"));
  if (
    sample === null ||
    contig === null ||
    start === undefined ||
    end === undefined ||
    end <= start
  )
    return undefined;
  if (mergedReference(sample, contig) === undefined) return undefined;
  return clampRegion({ sample, contig, start, end, context: 100 });
}

function restorePreferences(): void {
  const storedTheme = localStorage.getItem(THEME_KEY);
  if (storedTheme === "dark") {
    darkMode.value = true;
    document.documentElement.classList.add("dark");
  } else if (storedTheme === "light") {
    darkMode.value = false;
    document.documentElement.classList.remove("dark");
  } else {
    darkMode.value = false;
    document.documentElement.classList.remove("dark");
  }
  recentUrls.value = readStoredStrings(RECENT_URLS_KEY);
  recentSearches.value = readStoredStrings(RECENT_SEARCHES_KEY);
  try {
    const stored = JSON.parse(
      localStorage.getItem(LAYERS_KEY) ?? "null",
    ) as Partial<ViewerLayerState> | null;
    if (stored !== null) layers.value = { ...layers.value, ...stored };
  } catch {
    // Ignore invalid local preferences.
  }
}

function rememberRemoteUrl(value: string): void {
  const normalized = value.trim();
  if (normalized.length === 0) return;
  recentUrls.value = [
    normalized,
    ...recentUrls.value.filter((item) => item !== normalized),
  ].slice(0, 5);
  localStorage.setItem(RECENT_URLS_KEY, JSON.stringify(recentUrls.value));
}

function rememberSearch(value: string): void {
  if (value.length === 0) return;
  recentSearches.value = [
    value,
    ...recentSearches.value.filter((item) => item !== value),
  ].slice(0, 8);
  localStorage.setItem(
    RECENT_SEARCHES_KEY,
    JSON.stringify(recentSearches.value),
  );
}

function readStoredStrings(key: string): string[] {
  try {
    const value = JSON.parse(localStorage.getItem(key) ?? "[]") as unknown;
    return Array.isArray(value)
      ? value.filter((item): item is string => typeof item === "string")
      : [];
  } catch {
    return [];
  }
}

function clampRegion(region: RegionQuery): RegionQuery {
  const reference = mergedReference(region.sample, region.contig);
  if (reference === undefined) return region;
  const span = Math.min(
    reference.end - reference.start,
    Math.max(1, region.end - region.start),
  );
  const start = Math.max(
    reference.start,
    Math.min(reference.end - span, region.start),
  );
  return {
    sample: region.sample,
    contig: region.contig,
    start,
    end: start + span,
    context: region.context ?? 100,
  };
}

function mergedReference(
  sample: string,
  contig: string,
): ReferenceDescriptor | undefined {
  const matches = references.value.filter(
    (reference) => reference.sample === sample && reference.contig === contig,
  );
  if (matches.length === 0) return undefined;
  return {
    sample,
    contig,
    start: Math.min(...matches.map((reference) => reference.start)),
    end: Math.max(...matches.map((reference) => reference.end)),
    orientation: "forward",
  };
}

function predictedAdjacentRegion(region: RegionQuery): RegionQuery {
  const span = region.end - region.start;
  return clampRegion({
    ...region,
    start: region.end,
    end: region.end + span,
  });
}

function metricValue(bin: OverviewBin): bigint {
  return bin[summaryMetric.value];
}

function zeroBudgets() {
  return {
    compressedBytes: 0n,
    decodedBytes: 0n,
    nodeRecords: 0n,
    edgeRecords: 0n,
    occurrences: 0n,
  };
}

function detachViewer(): void {
  for (const unsubscribe of viewerUnsubscribers) unsubscribe();
  viewerUnsubscribers = [];
  viewer.value?.destroy();
  viewer.value = undefined;
}

function fail(cause: unknown, fallback: string): void {
  phase.value = "error";
  errorMessage.value = actionableError(cause);
  statusMessage.value = fallback;
}

function actionableError(cause: unknown): string {
  const message = cause instanceof Error ? cause.message : String(cause);
  if (/failed to fetch|cors|networkerror/i.test(message))
    return `${message} The origin must allow this site with CORS and expose the range headers.`;
  if (
    /206|content-range|range request|returned 200|full response/i.test(message)
  )
    return `${message} The origin must return exact 206 Partial Content responses; large whole-object fallbacks are rejected.`;
  if (/etag|object.*changed|identity/i.test(message))
    return `${message} Use an immutable URL with one stable strong ETag.`;
  if (/unsupported.*version|magic/i.test(message))
    return `${message} Regenerate the archive with the current v1 encoder.`;
  if (/integrity|corrupt|checksum|decompress/i.test(message))
    return `${message} The range failed archive integrity validation; retry only after checking the immutable object.`;
  return message;
}

function isAbort(cause: unknown): boolean {
  return cause instanceof DOMException && cause.name === "AbortError";
}

function safeInteger(value: string | null): number | undefined {
  if (value === null || !/^\d+$/.test(value)) return undefined;
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) ? parsed : undefined;
}

function formatShortRegion(region: RegionQuery): string {
  return `${region.sample} ${region.contig}:${formatCoordinate(region.start)}–${formatCoordinate(region.end)}`;
}

function formatCoordinate(value: number): string {
  return new Intl.NumberFormat("en-US", { maximumFractionDigits: 0 }).format(
    value,
  );
}

function formatBytes(value: number | bigint | undefined): string {
  if (value === undefined) return "—";
  const bytes = typeof value === "bigint" ? Number(value) : value;
  if (bytes < 1_024) return `${bytes} B`;
  if (bytes < 1_048_576) return `${(bytes / 1_024).toFixed(1)} KiB`;
  if (bytes < 1_073_741_824) return `${(bytes / 1_048_576).toFixed(1)} MiB`;
  return `${(bytes / 1_073_741_824).toFixed(2)} GiB`;
}

function formatMs(value: number | undefined): string {
  return value === undefined ? "—" : `${value.toFixed(1)} ms`;
}

function formatCompact(value: bigint | undefined): string {
  if (value === undefined) return "—";
  const numeric = Number(value);
  if (numeric < 1_000) return numeric.toLocaleString();
  if (numeric < 1_000_000) return `${Math.round(numeric / 1_000)}k`;
  return `${(numeric / 1_000_000).toFixed(1)}m`;
}
</script>

<template>
  <main class="explorer" :class="{ dark: darkMode, 'palette-open': commandPaletteOpen }" :data-phase="phase" :data-screen="screen">
    <ExplorerTopbar
      ref="commandBar"
      :command="command"
      :palette-open="commandPaletteOpen"
      :active-descendant="suggestions[activeSuggestion] === undefined ? undefined : `locus-option-${activeSuggestion}`"
      :archive-title="archiveTitle"
      :archive-size="formatBytes(archiveInfo?.archiveBytes)"
      :phase="phase"
      @update:command="command = $event"
      @focus="commandPaletteOpen = true; scheduleSuggestions()"
      @keydown="onCommandKeyDown"
      @submit="submitCommand"
      @copy-link="copyRegionLink"
      @toggle-theme="toggleTheme"
      @open-source="sourceOpen = !sourceOpen"
      @open-home="openShowcase"
    />

    <ToolRail
      :screen="screen"
      :active-panel="toolPanel"
      @home="openShowcase"
      @panel="toolPanel = toolPanel === $event ? null : $event"
      @source="sourceOpen = true"
      @help="shortcutsOpen = true"
    />

    <aside v-if="toolPanel" class="tool-popover" :aria-label="`${toolPanel} controls`">
      <header><strong>{{ toolPanel === 'navigate' ? 'Navigate' : toolPanel === 'layers' ? 'Graph layers' : toolPanel === 'tracks' ? 'Overview tracks' : 'Detail policy' }}</strong><button type="button" aria-label="Close controls" @click="toolPanel = null">Close</button></header>
      <template v-if="toolPanel === 'navigate'"><code>{{ canonicalCoordinate }}</code><div class="button-row"><button type="button" @click="panRegion(-0.35)">Pan left</button><button type="button" @click="zoomRegion(2)">Zoom out</button><button type="button" @click="zoomRegion(0.5)">Zoom in</button><button type="button" @click="panRegion(0.35)">Pan right</button></div><button type="button" class="text-button" @click="fitReference">Fit reference</button><button type="button" class="text-button" :disabled="selectedLocus === undefined" @click="fitLocus">Fit active locus</button></template>
      <template v-else-if="toolPanel === 'layers'"><label class="check"><input v-model="layers.reference" type="checkbox" /><span class="swatch reference"></span>Reference backbone</label><label class="check"><input v-model="layers.topology" type="checkbox" /><span class="swatch alternate"></span>Alternate topology</label><label class="check"><input v-model="layers.traversals" type="checkbox" /><span class="swatch traversal"></span>Local traversal evidence</label><label class="check"><input v-model="layers.tileBoundaries" type="checkbox" /><span class="swatch tile"></span>Source tile boundaries</label><label class="check"><input v-model="layers.sequenceLabels" type="checkbox" />Readable sequence labels</label><p class="semantics">Anonymous weighted traversals remain local to each source tile. They are not people, alleles, frequencies, or globally stitchable samples.</p></template>
      <template v-else-if="toolPanel === 'tracks'"><label class="field-label" for="summary-metric">Inspection metric</label><select id="summary-metric" v-model="summaryMetric"><option value="coveredBases">Covered reference bases</option><option value="tileCount">Regional tile count</option><option value="encodedBytes">Encoded regional bytes</option><option value="decodedBytes">Decoded regional bytes</option><option value="nodeRecords">Node-record count</option><option value="edgeRecords">Edge-record count</option><option value="gbwtRecords">GBWT-record count</option><option value="occurrences">Occurrence count</option></select><div class="segmented"><button v-for="scale in (['linear', 'log', 'normalized'] as const)" :key="scale" type="button" :aria-pressed="summaryScale === scale" @click="summaryScale = scale">{{ scale }}</button></div><p>Composite view always shows topology, traversal evidence, and exact planned transfer cost.</p></template>
      <template v-else><div class="mode-pill" :data-mode="displayMode">{{ displayMode }}</div><p>{{ lodDecision?.reason ?? 'Waiting for summary evidence.' }}</p><label class="check"><input v-model="forceDetail" type="checkbox" />Override the automatic request decision</label><p v-if="lodDecision?.usesPartialBinEstimates" class="semantics">Record counts are coverage-prorated whole-bin estimates. Transfer bytes and tile counts come from the exact directory plan.</p></template>
    </aside>

    <ShowcaseStage
      v-show="screen === 'showcase'"
      :locus="selectedLocus?.displayName ?? configuredDefaultLocus"
      :archive-size="formatBytes(archiveInfo?.archiveBytes)"
      :named-loci="formatCompact(archiveInfo?.namedLoci.recordCount)"
      :reference-paths="references.length.toLocaleString()"
      :detail-transfer="formatBytes(queryTrace?.payloadBytes ?? regionPlan?.compressedBytes)"
      :ready="phase === 'ready'"
      @open-detail="openShowcaseDetail"
      @open-overview="openShowcaseOverview"
      @open-search="commandPaletteOpen = true; commandBar?.focus()"
    />

    <OverviewStage
      v-show="screen === 'explorer' && graphSurfaceMode === 'hidden'"
      ref="overviewStage"
      :title="selectedLocus?.displayName ?? currentReference?.contig ?? 'Pangenome'"
      :coordinate="canonicalCoordinate"
      :region="overviewDisplayRegion"
      :locus="selectedLocus"
      :plan="overviewDisplayPlan"
      :bins="overviewDisplayBins"
      :archive-info="archiveInfo"
      :reference-count="references.length"
      :skeleton="phase === 'summary' && overviewDisplayBins.length === 0"
      :can-open-detail="recommendedDetailRegion !== undefined"
      :detail-reason="lodDecision?.automaticDetail ? 'This locus fits the bounded detail budget.' : 'Open a focused window to keep the graph legible.'"
      :archive-size="formatBytes(archiveInfo?.archiveBytes)"
      :base-bin-span="formatCoordinate(archiveInfo?.summaries?.baseBinSpan ?? 0) + ' bp'"
      :scale="summaryScale"
      @open-detail="openRecommendedDetail"
      @wheel="onSummaryWheel"
      @pointerdown="onSummaryPointerDown"
      @pointermove="onSummaryPointerMove"
      @pointerup="onSummaryPointerUp"
      @copy-coordinate="copyCoordinate"
    />

    <DetailedGraphStage
      :mode="graphSurfaceMode"
      :locus="selectedLocus?.displayName ?? currentReference?.contig ?? 'Pangenome'"
      :coordinate="canonicalCoordinate"
      :layers="layers"
      @toggle-topology="layers.topology = !layers.topology"
      @toggle-traversals="layers.traversals = !layers.traversals"
      @toggle-sequence="layers.sequenceLabels = !layers.sequenceLabels"
    ><div ref="viewerHost" class="viewer-host"></div></DetailedGraphStage>

    <LoadingCard
      :visible="loadingVisible"
      :phase="phase"
      :locus="selectedLocus?.displayName ?? currentReference?.contig ?? 'region'"
      :open-time="formatMs(openMs)"
      :summary-bytes="formatBytes(summaryTrace?.totalBytes)"
      :expected-transfer="formatBytes(regionPlan?.compressedBytes)"
      :remote-ranges="regionPlan?.selectedChunks ?? 0"
      :completed-tiles="progress?.counts.tiles ?? 0"
      :planned-tiles="regionPlan?.selectedChunks ?? 0"
      :reason="lodDecision?.automaticDetail ? 'Selected locus fits detail budgets' : 'Focused graph request'"
      @cancel="cancelCurrentLoad"
    />

    <InspectorDrawer v-if="screen === 'explorer'" :selection="inspector" @close="inspector = { kind: 'archive' }" @copy-sequence="copyNodeSequence" />

    <EvidenceSheet
      :open="evidenceOpen"
      :phase="phase"
      :label="phase === 'ready' ? (screen === 'showcase' ? 'Live archive ready' : detailVisible ? 'Detailed graph ready' : 'Overview ready') : statusMessage"
      :summary-trace="summaryTrace"
      :query-trace="queryTrace"
      :bytes="queryTrace?.payloadBytes ?? regionPlan?.compressedBytes ?? summaryTrace?.totalBytes"
      :tiles="regionPlan?.selectedChunks ?? 0"
      :wall-ms="queryWallMs"
      :open-ms="openMs"
      :summary-paint-ms="summaryPaintMs"
      :performance="viewerPerformance"
      :technical-mode="technicalMode"
      @toggle="evidenceOpen = !evidenceOpen"
      @update:technical-mode="technicalMode = $event"
    />

    <SearchPalette
      :open="commandPaletteOpen"
      :suggestions="suggestions"
      :active-suggestion="activeSuggestion"
      :recent-searches="recentSearches"
      :search-state="searchState"
      :search-message="searchMessage"
      @close="commandPaletteOpen = false"
      @activate="activeSuggestion = $event"
      @select="selectLocus"
      @recent="useRecentSearch"
    />

    <div class="sr-status" role="status" aria-live="polite">{{ statusMessage }}</div><div v-if="errorMessage" class="toast error-toast" role="alert"><strong>Explorer error</strong><span>{{ errorMessage }}</span><a :href="withBase('/HOSTING')">Origin requirements</a></div><div v-else-if="shareMessage" class="toast" role="status">{{ shareMessage }}</div>
    <dialog :open="sourceOpen" class="source-dialog" aria-label="Archive source"><header><div><span>Archive source</span><h2>Open a static pangenome object</h2></div><button type="button" @click="sourceOpen = false">Close</button></header><label><span>Source</span><select v-model="archiveChoice" aria-label="Archive source" @change="onSourceChange"><option value="configured" :disabled="configuredArchiveUrl.length === 0">Configured HPRC v2.1 + GENCODE v50</option><option value="population" :disabled="populationArchiveUrl.length === 0">1000 Genomes hs38d1 (NA19239#0)</option><option value="fixture">Bundled deterministic fixture</option><option value="custom">Custom remote URL</option><option value="local">Local .pngr file</option></select></label><p v-if="archiveChoice === 'population'" class="source-coordinate-note"><strong>Population-path coordinates:</strong> this archive follows real NA19239 haplotype-0 paths and is not GRCh38.</p><label v-if="archiveChoice === 'custom'"><span>Remote .pngr URL</span><input v-model="customUrl" aria-label="Remote archive URL" type="url" placeholder="https://archive.example/immutable.pngr" /></label><button v-if="archiveChoice === 'custom'" type="button" class="primary-button" @click="loadSource">Open remote archive</button><button v-if="archiveChoice === 'local'" type="button" class="primary-button" @click="localFileInput?.click()">Choose local file</button><input ref="localFileInput" class="visually-hidden" type="file" accept=".pngr,application/octet-stream" @change="onLocalFile" /><div class="archive-summary"><span class="source-dot" :data-state="phase"></span><div><strong>{{ archiveTitle }}</strong><small>{{ formatBytes(archiveInfo?.archiveBytes) }} · format v{{ archiveInfo?.formatVersion ?? '—' }}</small></div></div><dl><div><dt>Named loci</dt><dd>{{ archiveInfo?.namedLoci.state ?? '—' }} · {{ archiveInfo?.namedLoci.recordCount.toString() ?? '0' }}</dd></div><div><dt>Summary index</dt><dd>{{ archiveInfo?.summaries ? `${archiveInfo.summaries.baseBinSpan} bp base bins` : 'absent' }}</dd></div><div><dt>Semantics</dt><dd>{{ archiveInfo?.haplotypeSemantics ?? '—' }}</dd></div></dl><p>Custom URLs and local filenames stay in this browser. No query backend is used.</p></dialog>
    <dialog :open="shortcutsOpen" class="shortcut-dialog" aria-label="Keyboard shortcuts"><header><h2>Keyboard controls</h2><button type="button" @click="shortcutsOpen = false">Close</button></header><dl><div><dt><kbd>⌘/Ctrl K</kbd> or <kbd>/</kbd></dt><dd>Focus search</dd></div><div><dt><kbd>Left</kbd> <kbd>Right</kbd></dt><dd>Pan graph viewport</dd></div><div><dt><kbd>+</kbd> <kbd>−</kbd></dt><dd>Zoom graph viewport</dd></div><div><dt><kbd>Home</kbd></dt><dd>Reset local graph transform</dd></div><div><dt><kbd>?</kbd></dt><dd>Toggle this help</dd></div><div><dt><kbd>Esc</kbd></dt><dd>Close transient panels</dd></div></dl></dialog>
  </main>
</template>

<style src="./PangenomeDemo.css"></style>

<style>
body.pangenome-explorer-active {
  overflow: hidden;
}
body.pangenome-explorer-active .VPNav,
body.pangenome-explorer-active .VPSidebar,
body.pangenome-explorer-active .VPLocalNav,
body.pangenome-explorer-active footer {
  display: none;
}
body.pangenome-explorer-active .VPContent,
body.pangenome-explorer-active .VPPage {
  padding: 0;
}
body.pangenome-explorer-active .VPContent.has-sidebar {
  padding-left: 0;
}
body.pangenome-explorer-active .VPPage .container,
body.pangenome-explorer-active .VPPage .content,
body.pangenome-explorer-active .vp-doc {
  max-width: none;
  margin: 0;
}
body.pangenome-explorer-active .VPPage .content {
  padding: 0;
}
</style>
