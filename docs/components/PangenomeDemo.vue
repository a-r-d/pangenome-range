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
  type ViewerSelectionDetail,
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

type ArchiveChoice =
  | "fixture"
  | "configured"
  | "population"
  | "custom"
  | "local";
type Phase = "idle" | "opening" | "summary" | "graph" | "ready" | "error";
type SearchState =
  | "index-absent"
  | "index-empty"
  | "ready"
  | "searching"
  | "no-matches"
  | "results"
  | "truncated"
  | "failed";
type SummaryMetric =
  | "coveredBases"
  | "tileCount"
  | "encodedBytes"
  | "decodedBytes"
  | "nodeRecords"
  | "edgeRecords"
  | "gbwtRecords"
  | "occurrences";
type SummaryScale = "linear" | "log" | "normalized";
type InspectorSelection =
  | { kind: "archive" }
  | ViewerSelectionDetail
  | { kind: "summary"; bin: OverviewBin }
  | { kind: "locus"; hit: LocusHit; matchedAlias: boolean };

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
const RECENT_URLS_KEY = "pangenome-range:recent-urls:v1";
const RECENT_SEARCHES_KEY = "pangenome-range:recent-searches:v1";
const LAYERS_KEY = "pangenome-range:layers:v1";
const THEME_KEY = "pangenome-range:theme:v1";
const viewerHost = ref<HTMLElement>();
const summaryCanvas = ref<HTMLCanvasElement>();
const commandInput = ref<HTMLInputElement>();
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
const phase = ref<Phase>("idle");
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
const summaryTrace = shallowRef<FeatureQueryTrace>();
const queryTrace = shallowRef<QueryTrace>();
const progress = shallowRef<ViewerProgress>();
const lodDecision = shallowRef<ViewerLodDecision>();
const forceDetail = ref(false);
const summaryMetric = ref<SummaryMetric>("nodeRecords");
const summaryScale = ref<SummaryScale>("log");
const inspector = shallowRef<InspectorSelection>({ kind: "archive" });
const evidenceOpen = ref(false);
const sourceOpen = ref(false);
const shortcutsOpen = ref(false);
const technicalMode = ref(false);
const darkMode = ref(false);
const shareMessage = ref("");
const summaryPaintMs = ref<number>();
const openMs = ref<number>();
const queryWallMs = ref<number>();
const viewerPerformance = shallowRef<ViewerPerformanceSnapshot>();
const layers = ref<ViewerLayerState>({
  reference: true,
  topology: true,
  traversals: true,
  tileBoundaries: true,
  sequenceLabels: true,
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
    forceDetail.value ||
    lodDecision.value?.automaticDetail === true ||
    archiveInfo.value?.summaries === undefined,
);
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

onMounted(async () => {
  document.body.classList.add("pangenome-explorer-active");
  restorePreferences();
  restoreUrlState();
  window.addEventListener("popstate", onPopState);
  window.addEventListener("keydown", onGlobalKeyDown, { capture: true });
  await nextTick();
  if (summaryCanvas.value !== undefined) {
    summaryResizeObserver = new ResizeObserver(drawSummary);
    summaryResizeObserver.observe(summaryCanvas.value);
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
  errorMessage.value = "";
  sourceController?.abort();
  sourceController = new AbortController();
  regionController?.abort();
  searchController?.abort();
  const selected = selectedSource();
  sourceOpen.value = false;
  phase.value = "opening";
  statusMessage.value = `Opening ${selected.label}`;
  detachViewer();
  await archive.value?.close();
  archive.value = undefined;
  archiveInfo.value = undefined;
  selectedLocus.value = undefined;
  inspector.value = { kind: "archive" };
  summaryBins.value = [];
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
    archive.value = opened;
    activeSourceKey.value = selected.key;
    activeArchiveLabel.value = selected.label;
    references.value = opened.references();
    archiveInfo.value = await opened.info({ signal: sourceController.signal });
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
    command.value = formatGenomicCoordinate(
      initial.sample,
      initial.contig,
      initial.start,
      initial.end,
    );
    await navigateTo(initial, restored === undefined ? "replace" : "none");
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
    maxRenderedNodes: 2_000,
    maxRenderedEdges: 4_000,
    maxHaplotypeLanes: 24,
    showRequestTrace: false,
    initialLayers: layers.value,
    initialTheme: darkMode.value ? "dark" : "light",
  });
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
      const locus = selectedLocus.value;
      inspector.value =
        detail !== undefined
          ? detail
          : locus === undefined
            ? { kind: "archive" }
            : {
                kind: "locus",
                hit: locus,
                matchedAlias: locus.matchedName !== locus.displayName,
              };
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
        name: "HLA-B",
        mode: "exact",
        limit: 1,
      });
      const hit = result.hits[0];
      if (hit !== undefined) return paddedLocus(hit);
    } catch {
      // Coordinate exploration remains available if an optional index fails.
    }
  }
  const preferred =
    references.value.find(
      (reference) =>
        reference.sample === "GRCh38" && reference.contig === "chr1",
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
  queryWallMs.value = undefined;
  lodDecision.value = undefined;
  const started = performance.now();
  try {
    if (opened.capabilities().multiscaleSummaries) {
      phase.value = "summary";
      statusMessage.value = `Reading multiscale summary for ${formatShortRegion(region)}`;
      const summaryStarted = performance.now();
      const result = await opened.summary({
        ...region,
        maxBins: recommendedSummaryBins(
          summaryCanvas.value?.clientWidth ?? 900,
        ),
        signal,
        trace: true,
      });
      if (operation !== regionOperation) return;
      summaryBins.value = result.bins;
      summaryTrace.value = result.trace;
      lodDecision.value = chooseViewerLod(
        result.bins,
        region,
        summaryCanvas.value?.clientWidth ?? 900,
        DEFAULT_COMPLEXITY_BUDGETS,
        forceDetail.value,
      );
      await nextTick();
      drawSummary();
      summaryPaintMs.value = performance.now() - summaryStarted;
    } else {
      summaryBins.value = [];
      lodDecision.value = {
        mode: "detailed",
        automaticDetail: true,
        bpPerPixel:
          (region.end - region.start) /
          (summaryCanvas.value?.clientWidth ?? 900),
        estimates: zeroBudgets(),
        budgets: DEFAULT_COMPLEXITY_BUDGETS,
        limitingMetrics: [],
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
  if (suggestions.value.length === 0) {
    if (event.key === "Escape") suggestions.value = [];
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
  } else if (event.key === "Escape") {
    event.preventDefault();
    suggestions.value = [];
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
      selectedLocus.value = undefined;
      suggestions.value = [];
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

async function selectLocus(hit: LocusHit): Promise<void> {
  selectedLocus.value = hit;
  inspector.value = {
    kind: "locus",
    hit,
    matchedAlias: hit.matchedName !== hit.displayName,
  };
  suggestions.value = [];
  command.value = hit.displayName;
  rememberSearch(hit.displayName);
  await navigateTo(paddedLocus(hit));
}

function paddedLocus(hit: LocusHit): RegionQuery {
  const span = hit.reference.end - hit.reference.start;
  const padding = Math.max(1_000, Math.round(span * 0.15));
  return clampRegion({
    sample: hit.reference.sample,
    contig: hit.reference.contig,
    start: Math.max(0, hit.reference.start - padding),
    end: hit.reference.end + padding,
    context: 100,
  });
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
  const canvas = summaryCanvas.value;
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
  summaryCanvas.value?.setPointerCapture?.(event.pointerId);
  if (summaryPointers.size === 2 && pointerStartRegion !== undefined) {
    const [first, second] = [...summaryPointers.values()];
    const canvas = summaryCanvas.value;
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
  const canvas = summaryCanvas.value;
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
  summaryCanvas.value?.releasePointerCapture?.(event.pointerId);
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
  const canvas = summaryCanvas.value;
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
  const canvas = summaryCanvas.value;
  const region = visualRegion.value ?? activeRegion.value;
  if (canvas === undefined || region === undefined) return;
  const width = Math.max(320, canvas.clientWidth || 900);
  const height = Math.max(140, canvas.clientHeight || 190);
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
        background: "#10151d",
        grid: "#293342",
        text: "#9aaabd",
        accent: "#52c7bd",
        coverage: "#7767c5",
      }
    : {
        background: "#fbfcfd",
        grid: "#dfe5eb",
        text: "#647386",
        accent: "#087f75",
        coverage: "#7367b8",
      };
  context.fillStyle = colors.background;
  context.fillRect(0, 0, width, height);
  context.font = "11px ui-monospace, SFMono-Regular, Menlo, monospace";
  context.fillStyle = colors.text;
  context.fillText(
    `${metricLabel.value} · ${summaryScale.value} scale`,
    16,
    20,
  );
  const plot = { left: 16, top: 38, width: width - 32, height: height - 68 };
  context.strokeStyle = colors.grid;
  context.beginPath();
  context.moveTo(plot.left, plot.top + plot.height);
  context.lineTo(plot.left + plot.width, plot.top + plot.height);
  context.stroke();
  const visible = summaryBins.value.filter(
    (bin) =>
      bin.reference.start < region.end && bin.reference.end > region.start,
  );
  const values = visible.map((bin) => Number(metricValue(bin)));
  const maximum = Math.max(1, ...values);
  for (let index = 0; index < visible.length; index += 1) {
    const bin = visible[index] as OverviewBin;
    const raw = values[index] ?? 0;
    const scaled =
      summaryScale.value === "log"
        ? Math.log1p(raw) / Math.log1p(maximum)
        : raw / maximum;
    const x1 =
      plot.left +
      ((bin.reference.start - region.start) / (region.end - region.start)) *
        plot.width;
    const x2 =
      plot.left +
      ((bin.reference.end - region.start) / (region.end - region.start)) *
        plot.width;
    const barHeight = Math.max(1, scaled * plot.height);
    context.fillStyle = colors.accent;
    context.globalAlpha = 0.82;
    context.fillRect(
      x1,
      plot.top + plot.height - barHeight,
      Math.max(1, x2 - x1 - 1),
      barHeight,
    );
    const coverage = Number(bin.coveredBases) / Math.max(1, bin.binSpan);
    context.fillStyle = colors.coverage;
    context.globalAlpha = 0.9;
    context.fillRect(
      x1,
      height - 20,
      Math.max(1, x2 - x1),
      Math.max(2, coverage * 6),
    );
  }
  context.globalAlpha = 1;
  context.fillStyle = colors.text;
  context.textAlign = "left";
  context.fillText(formatCoordinate(region.start), plot.left, height - 6);
  context.textAlign = "right";
  context.fillText(
    formatCoordinate(region.end),
    plot.left + plot.width,
    height - 6,
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
  viewer.value?.setTheme(theme);
  drawSummary();
}

function onGlobalKeyDown(event: KeyboardEvent): void {
  const target = event.target as HTMLElement | null;
  const typing =
    target?.matches("input, textarea, select, [contenteditable=true]") === true;
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
    event.preventDefault();
    event.stopImmediatePropagation();
    commandInput.value?.focus();
  } else if (event.key === "/" && !typing) {
    event.preventDefault();
    commandInput.value?.focus();
  } else if (event.key === "?" && !typing) {
    shortcutsOpen.value = !shortcutsOpen.value;
  } else if (event.key === "Escape") {
    suggestions.value = [];
    sourceOpen.value = false;
    shortcutsOpen.value = false;
  }
}

function onPopState(): void {
  const region = regionFromUrl();
  if (region !== undefined) void navigateTo(region, "none");
}

function updateUrlState(mode: "push" | "replace"): void {
  if (archiveChoice.value === "local") return;
  const region = activeRegion.value;
  if (region === undefined) return;
  const params = new URLSearchParams();
  params.set("archive", archiveChoice.value);
  if (archiveChoice.value === "custom")
    params.set("url", customUrl.value.trim());
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
    darkMode.value = document.documentElement.classList.contains("dark");
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

function formatStrand(value: LocusHit["strand"]): string {
  return value === "forward" ? "+" : value === "reverse" ? "−" : "unknown";
}
</script>

<template>
  <main class="explorer" :class="{ dark: darkMode }" :data-phase="phase">
    <header class="topbar">
      <a class="brand" :href="withBase('/')" aria-label="pangenome-range documentation">
        <span class="brand-mark" aria-hidden="true">P</span>
        <span><strong>pangenome-range</strong><small>Explorer</small></span>
      </a>
      <form class="command" role="search" @submit.prevent="submitCommand">
        <span class="command-icon" aria-hidden="true">⌕</span>
        <input ref="commandInput" v-model="command" aria-label="Go to a locus or genomic coordinate" aria-autocomplete="list" :aria-activedescendant="suggestions[activeSuggestion] === undefined ? undefined : `locus-option-${activeSuggestion}`" autocomplete="off" placeholder="Gene, alias, or sample#contig:start-end" @focus="scheduleSuggestions" @keydown="onCommandKeyDown" />
        <kbd>⌘K</kbd><button type="submit">Go</button>
        <div v-if="suggestions.length > 0 || (namedCommandActive && ['index-absent', 'index-empty', 'searching', 'no-matches', 'truncated', 'failed'].includes(searchState))" class="suggestions" role="listbox" aria-label="Locus suggestions">
          <div v-if="searchState === 'searching'" class="suggestion-state">Searching archive locus pages…</div>
          <button v-for="(hit, index) in suggestions" :id="`locus-option-${index}`" :key="`${hit.stableId}:${hit.reference.sample}:${hit.reference.start}:${hit.matchedName}`" type="button" role="option" :aria-selected="index === activeSuggestion" @mouseenter="activeSuggestion = index" @click="selectLocus(hit)">
            <span><strong>{{ hit.displayName }}</strong><em v-if="hit.matchedName !== hit.displayName">matched alias {{ hit.matchedName }}</em></span>
            <span>{{ hit.featureType }} · {{ hit.stableId }}</span>
            <span>{{ hit.reference.sample }} · {{ hit.reference.contig }}:{{ formatCoordinate(hit.reference.start) }}–{{ formatCoordinate(hit.reference.end) }} · {{ formatStrand(hit.strand) }}</span>
          </button>
          <div v-if="searchState === 'index-absent'" class="suggestion-state">This archive has no named-locus index. Coordinate navigation remains available.</div>
          <div v-else-if="searchState === 'index-empty'" class="suggestion-state">The named-locus index is present but contains no records.</div>
          <div v-else-if="searchState === 'truncated'" class="suggestion-state">First {{ suggestions.length }} results shown · archive result limit reached</div>
          <div v-else-if="searchState === 'no-matches'" class="suggestion-state">No matching archive locus</div>
          <div v-else-if="searchState === 'failed'" class="suggestion-state error-text">{{ searchMessage }}</div>
        </div>
      </form>
      <nav class="top-actions" aria-label="Explorer actions">
        <button type="button" class="icon-button" title="Copy region link" @click="copyRegionLink">↗</button>
        <button type="button" class="icon-button" title="Toggle color theme" @click="toggleTheme">◐</button>
        <button type="button" class="icon-button" title="Keyboard help" @click="shortcutsOpen = !shortcutsOpen">?</button>
        <button type="button" class="source-button" @click="sourceOpen = !sourceOpen"><span class="source-dot" :data-state="phase"></span><span>{{ archiveTitle }}</span><small>{{ formatBytes(archiveInfo?.archiveBytes) }}</small></button>
      </nav>
    </header>

    <aside class="left-panel" aria-label="Layers and display controls">
      <section><h2>Viewport</h2><div class="coordinate-readout">{{ canonicalCoordinate }}</div><div class="button-row"><button type="button" title="Pan left" @click="panRegion(-0.35)">←</button><button type="button" title="Zoom out" @click="zoomRegion(2)">−</button><button type="button" title="Zoom in" @click="zoomRegion(0.5)">+</button><button type="button" title="Pan right" @click="panRegion(0.35)">→</button></div><button type="button" class="text-button" @click="fitReference">Fit reference</button><button type="button" class="text-button" :disabled="selectedLocus === undefined" @click="fitLocus">Fit active locus</button></section>
      <section><h2>Graph layers</h2><label class="check"><input v-model="layers.reference" type="checkbox" /><span class="swatch reference"></span>Reference traversal</label><label class="check"><input v-model="layers.topology" type="checkbox" /><span class="swatch alternate"></span>Alternate topology</label><label class="check"><input v-model="layers.traversals" type="checkbox" /><span class="swatch traversal"></span>Local traversals</label><label class="check"><input v-model="layers.tileBoundaries" type="checkbox" /><span class="swatch tile"></span>Tile boundaries</label><label class="check"><input v-model="layers.sequenceLabels" type="checkbox" />Sequence labels</label></section>
      <section><h2>Summary track</h2><label class="field-label" for="summary-metric">Tile-record metric</label><select id="summary-metric" v-model="summaryMetric"><option value="coveredBases">Covered reference bases</option><option value="tileCount">Regional tile count</option><option value="encodedBytes">Encoded regional bytes</option><option value="decodedBytes">Decoded regional bytes</option><option value="nodeRecords">Node-record count</option><option value="edgeRecords">Edge-record count</option><option value="gbwtRecords">GBWT-record count</option><option value="occurrences">Occurrence count</option></select><div class="segmented"><button v-for="scale in (['linear', 'log', 'normalized'] as const)" :key="scale" type="button" :aria-pressed="summaryScale === scale" @click="summaryScale = scale">{{ scale }}</button></div></section>
      <section><h2>Detail policy</h2><div class="mode-pill" :data-mode="displayMode">{{ displayMode }}</div><p>{{ lodDecision?.reason ?? 'Waiting for summary evidence.' }}</p><label class="check"><input v-model="forceDetail" type="checkbox" />Override automatic decision</label></section>
      <section class="semantics"><h2>Evidence semantics</h2><p>Anonymous weighted traversals remain local to each source tile. They are not people, alleles, frequencies, or globally stitchable samples.</p></section>
    </aside>

    <section class="workspace" aria-label="Genomic viewport">
      <div class="viewport-header"><div><span class="mode-label">{{ displayMode }} mode</span><strong>{{ canonicalCoordinate }}</strong></div><div class="stage-progress" aria-hidden="true"><span :class="{ active: phase === 'opening', done: !['idle', 'opening'].includes(phase) }">Open</span><span :class="{ active: phase === 'summary', done: ['graph', 'ready'].includes(phase) }">Summary</span><span :class="{ active: phase === 'graph', done: phase === 'ready' && detailVisible }">Graph</span><span :class="{ done: phase === 'ready' }">Ready</span></div></div>
      <div class="summary-stage" :class="{ skeleton: phase === 'summary' && summaryBins.length === 0 }"><canvas ref="summaryCanvas" aria-label="Multiscale archive summary; drag to pan, wheel to zoom, click a bin to inspect" tabindex="0" @wheel="onSummaryWheel" @pointerdown="onSummaryPointerDown" @pointermove="onSummaryPointerMove" @pointerup="onSummaryPointerUp" @pointercancel="onSummaryPointerUp"></canvas><div class="summary-caption"><span>{{ summaryBins.length }} bins · {{ metricLabel }}</span><span v-if="summaryBins[0]">level {{ summaryBins[0].level }} · {{ formatCoordinate(summaryBins[0].binSpan) }} bp/bin</span><span v-else-if="archiveInfo?.summaries === undefined">Summary index absent</span></div></div>
      <div class="detail-stage" :class="{ hidden: !detailVisible }"><div v-if="!detailVisible" class="detail-placeholder"><strong>Detailed payloads were not requested.</strong><p>{{ lodDecision?.reason }}</p><button type="button" @click="forceDetail = true">Load detailed graph</button></div><div ref="viewerHost" class="viewer-host"></div><div v-if="phase === 'graph'" class="loading-overlay" aria-hidden="true"><span></span>Streaming verified graph tiles…</div></div>
      <div class="scale-footer"><span>{{ formatCoordinate((visualRegion?.end ?? 0) - (visualRegion?.start ?? 0)) }} bp visible</span><span class="scale-line"></span><span>{{ lodDecision?.bpPerPixel.toFixed(1) ?? '—' }} bp / px</span><button type="button" @click="copyCoordinate">Copy coordinate</button></div>
    </section>

    <aside class="right-panel" aria-label="Selection inspector">
      <header><span>Inspector</span><button type="button" title="Show archive information" @click="inspector = { kind: 'archive' }">Archive</button></header>
      <section v-if="inspector.kind === 'node'">
        <p class="inspector-kind">Graph node</p><h2>Node {{ inspector.node.id.toString() }}</h2>
        <dl>
          <div><dt>Sequence length</dt><dd>{{ inspector.node.sequenceLength }} bp</dd></div>
          <div><dt>Orientation</dt><dd>{{ inspector.node.reverse ? 'reverse' : 'forward' }}</dd></div>
          <div><dt>Classification</dt><dd>{{ inspector.node.branchKind }}</dd></div>
          <div><dt>Branch lane</dt><dd>{{ inspector.node.lane }}</dd></div>
          <div><dt>Anchor interval</dt><dd>{{ formatCoordinate(inspector.node.anchorStart) }}–{{ formatCoordinate(inspector.node.anchorEnd) }}</dd></div>
          <div><dt>Neighbors</dt><dd>{{ inspector.incoming.length }} incoming · {{ inspector.outgoing.length }} outgoing</dd></div>
          <div><dt>Local traversal evidence</dt><dd>{{ inspector.localTraversalWeights.length }} weighted records</dd></div>
          <div><dt>Source tiles</dt><dd>{{ inspector.node.sourceTiles.length }}</dd></div>
        </dl>
        <h3>Sequence preview</h3><code class="sequence">{{ inspector.node.sequence.slice(0, 240) }}{{ inspector.node.sequence.length > 240 ? '…' : '' }}</code><button type="button" class="primary-button" @click="copyNodeSequence">Copy sequence</button>
        <h3>Payload provenance</h3><ul class="plain-list"><li v-for="source in inspector.node.sourceTiles" :key="source.archiveOffset.toString()">{{ formatCoordinate(source.coreStart) }}–{{ formatCoordinate(source.coreEnd) }} · byte {{ source.archiveOffset.toString() }} + {{ formatBytes(source.compressedBytes) }}</li></ul>
      </section>
      <section v-else-if="inspector.kind === 'edge'">
        <p class="inspector-kind">Graph edge</p><h2>{{ inspector.edge.from.toString() }} → {{ inspector.edge.to.toString() }}</h2>
        <dl><div><dt>Classification</dt><dd>{{ inspector.edge.classification }}</dd></div><div><dt>From orientation</dt><dd>{{ inspector.edge.fromReverse ? 'reverse' : 'forward' }}</dd></div><div><dt>To orientation</dt><dd>{{ inspector.edge.toReverse ? 'reverse' : 'forward' }}</dd></div><div><dt>Source tiles</dt><dd>{{ inspector.edge.sourceTiles.length }}</dd></div></dl>
        <ul class="plain-list"><li v-for="source in inspector.edge.sourceTiles" :key="source.archiveOffset.toString()">{{ formatCoordinate(source.coreStart) }}–{{ formatCoordinate(source.coreEnd) }} · byte {{ source.archiveOffset.toString() }} + {{ formatBytes(source.compressedBytes) }}</li></ul>
      </section>
      <section v-else-if="inspector.kind === 'traversal'">
        <p class="inspector-kind">Tile-local traversal</p><h2>Weight {{ inspector.traversal.weight.toString() }}</h2>
        <p class="explanation">Anonymous traversal evidence from one source tile. It is not a named individual and is never stitched across tiles.</p>
        <dl><div><dt>Core interval</dt><dd>{{ formatCoordinate(inspector.traversal.tileStart) }}–{{ formatCoordinate(inspector.traversal.tileEnd) }}</dd></div><div><dt>Oriented nodes</dt><dd>{{ inspector.traversal.orientedNodes.length }}</dd></div><div><dt>Compressed payload</dt><dd>{{ formatBytes(inspector.traversal.source.compressedBytes) }}</dd></div><div><dt>Decoded payload</dt><dd>{{ formatBytes(inspector.traversal.source.uncompressedBytes) }}</dd></div><div><dt>Archive range</dt><dd>{{ inspector.traversal.source.archiveOffset.toString() }} + {{ formatBytes(inspector.traversal.source.compressedBytes) }}</dd></div></dl>
        <code class="sequence">{{ inspector.traversal.orientedNodes.map((handle) => handle.toString()).join(' ') }}</code>
      </section>
      <section v-else-if="inspector.kind === 'summary'"><p class="inspector-kind">Summary bin</p><h2>{{ inspector.bin.reference.contig }}:{{ formatCoordinate(inspector.bin.reference.start) }}–{{ formatCoordinate(inspector.bin.reference.end) }}</h2><dl><div><dt>Level / span</dt><dd>{{ inspector.bin.level }} / {{ formatCoordinate(inspector.bin.binSpan) }} bp</dd></div><div><dt>Covered bases</dt><dd>{{ inspector.bin.coveredBases.toString() }}</dd></div><div><dt>Regional tiles</dt><dd>{{ inspector.bin.tileCount.toString() }}</dd></div><div><dt>Encoded bytes</dt><dd>{{ formatBytes(inspector.bin.encodedBytes) }}</dd></div><div><dt>Decoded bytes</dt><dd>{{ formatBytes(inspector.bin.decodedBytes) }}</dd></div><div><dt>Node records</dt><dd>{{ inspector.bin.nodeRecords.toString() }}</dd></div><div><dt>Edge records</dt><dd>{{ inspector.bin.edgeRecords.toString() }}</dd></div><div><dt>GBWT records</dt><dd>{{ inspector.bin.gbwtRecords.toString() }}</dd></div><div><dt>Occurrences</dt><dd>{{ inspector.bin.occurrences.toString() }}</dd></div></dl><p class="explanation">These are exact tile-record totals. They are not unique variants, allele frequencies, or individual counts.</p></section>
      <section v-else-if="inspector.kind === 'locus'"><p class="inspector-kind">Named locus</p><h2>{{ inspector.hit.displayName }}</h2><dl><div v-if="inspector.matchedAlias"><dt>Matched alias</dt><dd>{{ inspector.hit.matchedName }}</dd></div><div><dt>Stable ID</dt><dd>{{ inspector.hit.stableId }}</dd></div><div><dt>Feature type</dt><dd>{{ inspector.hit.featureType }}</dd></div><div><dt>Strand</dt><dd>{{ formatStrand(inspector.hit.strand) }}</dd></div><div><dt>Reference</dt><dd>{{ inspector.hit.reference.sample }}</dd></div><div><dt>Coordinates</dt><dd>{{ inspector.hit.reference.contig }}:{{ formatCoordinate(inspector.hit.reference.start) }}–{{ formatCoordinate(inspector.hit.reference.end) }}</dd></div><div><dt>Annotation</dt><dd>{{ archiveInfo?.provenance?.annotationRelease ?? 'declared index' }}</dd></div></dl></section>
      <section v-else><p class="inspector-kind">Archive</p><h2>{{ archiveTitle }}</h2><p>{{ archiveInfo?.provenance?.datasetDescription ?? 'Static range-addressable pangenome archive.' }}</p><dl><div><dt>Format</dt><dd>v{{ archiveInfo?.formatVersion ?? '—' }}</dd></div><div><dt>Object size</dt><dd>{{ formatBytes(archiveInfo?.archiveBytes) }}</dd></div><div><dt>Object identity</dt><dd class="truncate" :title="archiveInfo?.strongRemoteIdentity">{{ archiveInfo?.strongRemoteIdentity ?? 'local object' }}</dd></div><div><dt>References</dt><dd>{{ archiveInfo?.references.length ?? 0 }}</dd></div><div><dt>Named loci</dt><dd>{{ archiveInfo?.namedLoci.state ?? '—' }} · {{ archiveInfo?.namedLoci.recordCount.toString() ?? '0' }}</dd></div><div><dt>Summary index</dt><dd>{{ archiveInfo?.summaries ? `${archiveInfo.summaries.baseBinSpan} bp base bins` : 'absent' }}</dd></div><div><dt>Reference assembly</dt><dd>{{ archiveInfo?.provenance?.referenceAssembly ?? 'not declared' }}</dd></div><div><dt>Annotation</dt><dd>{{ archiveInfo?.provenance?.annotationRelease ?? 'not declared' }}</dd></div><div><dt>Semantics</dt><dd>{{ archiveInfo?.haplotypeSemantics ?? '—' }}</dd></div></dl><h3>Available reference samples</h3><div class="chips"><span v-for="sample in [...new Set(references.map((reference) => reference.sample))]" :key="sample">{{ sample }}</span></div></section>
    </aside>

    <section class="evidence" :class="{ open: evidenceOpen }" aria-label="Range and performance evidence">
      <button type="button" class="evidence-toggle" :aria-expanded="evidenceOpen" @click="evidenceOpen = !evidenceOpen">
        <span>Range & performance</span>
        <strong>{{ queryTrace?.requestRanges.length ?? summaryTrace?.requestRanges.length ?? 0 }} reads · {{ formatBytes(queryTrace?.totalBytes ?? summaryTrace?.totalBytes) }}</strong>
        <span>{{ formatMs(queryWallMs) }} wall</span>
        <span aria-hidden="true">{{ evidenceOpen ? '⌄' : '⌃' }}</span>
      </button>
      <div v-if="evidenceOpen" class="evidence-body">
        <div class="timing-strip">
          <article><span>Object open</span><strong>{{ formatMs(openMs) }}</strong></article>
          <article><span>First summary paint</span><strong>{{ formatMs(summaryPaintMs) }}</strong></article>
          <article><span>First graph tile</span><strong>{{ formatMs(viewerPerformance?.firstTilePaintMs) }}</strong></article>
          <article><span>Query complete</span><strong>{{ formatMs(queryWallMs) }}</strong></article>
          <article><span>Layout</span><strong>{{ formatMs(viewerPerformance?.layoutMs) }}</strong></article>
          <article><span>Paint</span><strong>{{ formatMs(viewerPerformance?.paintMs) }}</strong></article>
          <article><span>Frame p95</span><strong>{{ formatMs(viewerPerformance?.frameP95Ms) }}</strong></article>
        </div>
        <div class="waterfall">
          <div v-for="(range, index) in [...(summaryTrace?.requestRanges ?? []), ...(queryTrace?.requestRanges ?? [])]" :key="`${range.offset}:${range.length}:${index}`">
            <span>{{ range.layer }}</span><i :style="{ width: `${Math.max(2, Math.min(100, (range.length / Math.max(1, queryTrace?.totalBytes ?? summaryTrace?.totalBytes ?? range.length)) * 100))}%` }"></i><code>{{ range.offset.toString() }} + {{ formatBytes(range.length) }}</code>
          </div>
          <p v-if="summaryTrace === undefined && queryTrace === undefined">No traced request has completed yet.</p>
        </div>
        <div class="evidence-meta">
          <span>Visual {{ visualRegion ? formatShortRegion(visualRegion) : '—' }}</span>
          <span>Requested {{ requestedRegion ? formatShortRegion(requestedRegion) : '—' }}</span>
          <span>Loaded {{ loadedRegion ? formatShortRegion(loadedRegion) : '—' }}</span>
          <span>Predicted adjacent {{ prefetchedRegion ? formatShortRegion(prefetchedRegion) : '—' }} (not fetched)</span>
          <span v-if="queryTrace">Canonical hash {{ queryTrace.canonicalHash }}</span>
        </div>
        <label class="check"><input v-model="technicalMode" type="checkbox" />Technical evidence mode</label>
        <p v-if="technicalMode" class="technical-note">Integrity, decompression, and decode timings are displayed independently and are never added together as elapsed wall time. Current reader fields: integrity {{ formatMs(queryTrace?.integrityMs) }}, decompression interval-union wall {{ formatMs(queryTrace?.decompressionMs) }}, decompression task aggregate {{ formatMs(queryTrace?.decompressionTaskMs) }}, regional decode {{ formatMs(queryTrace?.decodeMs) }}, graph merge {{ formatMs(queryTrace?.mergeMs) }}.</p>
      </div>
    </section>

    <div class="sr-status" role="status" aria-live="polite">{{ statusMessage }}</div>
    <div v-if="errorMessage" class="toast error-toast" role="alert"><strong>Explorer error</strong><span>{{ errorMessage }}</span><a :href="withBase('/HOSTING')">Origin requirements</a></div><div v-else-if="shareMessage" class="toast" role="status">{{ shareMessage }}</div>
    <dialog :open="sourceOpen" class="source-dialog" aria-label="Archive source"><header><h2>Open archive</h2><button type="button" @click="sourceOpen = false">×</button></header><label><span>Source</span><select v-model="archiveChoice" aria-label="Archive source" @change="onSourceChange"><option value="configured" :disabled="configuredArchiveUrl.length === 0">HPRC v2.1 + GENCODE v50 (GRCh38 / CHM13)</option><option value="population" :disabled="populationArchiveUrl.length === 0">1000 Genomes hs38d1 (NA19239#0, no annotations)</option><option value="fixture">Bundled deterministic fixture</option><option value="custom">Custom remote URL</option><option value="local">Local .pngr file</option></select></label><p v-if="archiveChoice === 'population'" class="source-coordinate-note"><strong>Population-path coordinates:</strong> this archive follows the real NA19239 haplotype-0 paths. It has no named-locus annotations and is not GRCh38.</p><label v-if="archiveChoice === 'custom'"><span>Remote .pngr URL</span><input v-model="customUrl" aria-label="Remote archive URL" type="url" placeholder="https://archive.example/immutable.pngr" /></label><button v-if="archiveChoice === 'custom'" type="button" class="primary-button" @click="loadSource">Open remote archive</button><button v-if="archiveChoice === 'local'" type="button" class="primary-button" @click="localFileInput?.click()">Choose local file</button><input ref="localFileInput" class="visually-hidden" type="file" accept=".pngr,application/octet-stream" @change="onLocalFile" /><div v-if="recentUrls.length > 0" class="recent-list"><h3>Recently opened on this device</h3><button v-for="url in recentUrls" :key="url" type="button" @click="useRecentUrl(url)">{{ url }}</button></div><p>Custom URLs and local filenames stay in this browser. The application has no analytics or query backend.</p></dialog>
    <dialog :open="shortcutsOpen" class="shortcut-dialog" aria-label="Keyboard shortcuts"><header><h2>Keyboard controls</h2><button type="button" @click="shortcutsOpen = false">×</button></header><dl><div><dt><kbd>⌘/Ctrl K</kbd> or <kbd>/</kbd></dt><dd>Focus command bar</dd></div><div><dt><kbd>←</kbd> <kbd>→</kbd></dt><dd>Pan graph viewport</dd></div><div><dt><kbd>+</kbd> <kbd>−</kbd></dt><dd>Zoom graph viewport</dd></div><div><dt><kbd>Home</kbd></dt><dd>Reset local graph transform</dd></div><div><dt><kbd>?</kbd></dt><dd>Toggle this help</dd></div><div><dt><kbd>Esc</kbd></dt><dd>Close transient panels</dd></div></dl></dialog>
  </main>
</template>

<style scoped src="./PangenomeDemo.css"></style>

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
