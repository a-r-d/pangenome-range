<script setup lang="ts">
// biome-ignore-all lint/correctness/noUnusedVariables: Vue template bindings are consumed outside the script AST.
import {
  type ArchiveInfo,
  type LocusHit,
  type OverviewBin,
  openPangenome,
  type PangenomeArchive,
  type QueryTrace,
  type ReferenceDescriptor,
  type RegionPlan,
  type RegionQuery,
  type RegionTile,
} from "pangenome-range/reader";
import {
  buildTubeMapModel,
  decideGraphRegion,
  EXTENDED_TUBE_MAP_DISPLAY_LIMITS,
  formatGenomicCoordinate,
  parseGenomicCommand,
  recommendedGraphRegion,
  type TubeMapModel,
} from "pangenome-range/viewer";
import { withBase } from "vitepress";
import {
  computed,
  onBeforeUnmount,
  onMounted,
  ref,
  shallowRef,
  watch,
} from "vue";
import ArchiveSourceMenu from "./ArchiveSourceMenu.vue";
import BrowserStatusBar from "./BrowserStatusBar.vue";
import BrowserToolbar from "./BrowserToolbar.vue";
import LinearReferenceTrack from "./LinearReferenceTrack.vue";
import NodeInspector from "./NodeInspector.vue";
import PatternInspector from "./PatternInspector.vue";
import ShareDialog from "./ShareDialog.vue";
// biome-ignore lint/style/useImportType: the Vue template needs the runtime component.
import TubeMapView from "./TubeMapView.vue";
import type {
  ArchiveSourceSelection,
  BrowserMetrics,
  BrowserPhase,
  BrowserSelection,
  DemoArchiveId,
  GraphOptions,
  GraphViewport,
} from "./types";
import "./browser.css";

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
const configuredPadding = Math.max(
  0,
  Number.parseInt(
    (import.meta.env.VITE_PANGENOME_RANGE_DEMO_DEFAULT_PADDING as
      | string
      | undefined) ?? "2000",
    10,
  ) || 2_000,
);
const fixtureUrl = withBase("/fixtures/format-v1.pngr");
const root = ref<HTMLElement>();
const tubeMap = ref<InstanceType<typeof TubeMapView>>();
const archive = shallowRef<PangenomeArchive>();
const info = shallowRef<ArchiveInfo>();
const references = ref<readonly ReferenceDescriptor[]>([]);
const region = shallowRef<RegionQuery>();
const locus = shallowRef<LocusHit>();
const plan = shallowRef<RegionPlan>();
const bins = shallowRef<readonly OverviewBin[]>([]);
const tiles = shallowRef<readonly RegionTile[]>([]);
const model = shallowRef<TubeMapModel>();
const trace = shallowRef<QueryTrace>();
const phase = ref<BrowserPhase>("opening");
const message = ref("Opening the configured static archive");
const command = ref("");
const suggestions = ref<readonly LocusHit[]>([]);
const activeSuggestion = ref(0);
const searching = ref(false);
const searchMessage = ref("");
const selection = shallowRef<BrowserSelection>();
const sourceOpen = ref(false);
const shareOpen = ref(false);
const shareUrl = ref("");
const activeSourceLabel = ref("Configured HPRC archive");
const activeSourceId = ref<DemoArchiveId>("hprc");
const activeSourceKey = ref("");
const activeCustomUrl = ref("");
const expandedGroups = ref<readonly string[]>([]);
const extendedDisplayBudget = ref(false);
const viewport = shallowRef<GraphViewport>();
const options = ref<GraphOptions>({
  patternCount: 8,
  simplifyLinearChains: true,
  showBases: "automatic",
  showTileBoundaries: true,
});
const metrics = ref<BrowserMetrics>({});
let sourceOperation = 0;
let regionOperation = 0;
let sourceController: AbortController | undefined;
let regionController: AbortController | undefined;
let searchController: AbortController | undefined;
let searchTimer: ReturnType<typeof setTimeout> | undefined;
let suppressedCommand: string | undefined;
let viewportUrlFrame: number | undefined;

const configuredLabel = "HPRC v2.1 + GENCODE v50 (GRCh38 / CHM13)";
const populationLabel = "1000 Genomes hs38d1 (NA19239 haplotype 0)";
const demoSources = computed<readonly ArchiveSourceSelection[]>(() => {
  const sources: ArchiveSourceSelection[] = [];
  if (configuredArchiveUrl.length > 0) {
    sources.push({
      id: "hprc",
      source: configuredArchiveUrl,
      label: configuredLabel,
      key: `url:${configuredArchiveUrl}`,
      description:
        "Whole HPRC v2.1 graph with GRCh38 and CHM13 references plus GENCODE v50 named-gene search.",
    });
  }
  if (populationArchiveUrl.length > 0) {
    sources.push({
      id: "1000g",
      source: populationArchiveUrl,
      label: populationLabel,
      key: `url:${populationArchiveUrl}`,
      description:
        "NA19239 haplotype-0 population-path coordinates. This archive has no named-gene annotations and is not GRCh38.",
    });
  }
  sources.push({
    id: "fixture",
    source: fixtureUrl,
    label: "Bundled deterministic fixture",
    key: `url:${fixtureUrl}`,
    description: "Tiny offline fixture for deterministic reader testing.",
  });
  return sources;
});

const decision = computed(() => {
  const currentRegion = region.value;
  const currentPlan = plan.value;
  return currentRegion === undefined || currentPlan === undefined
    ? undefined
    : decideGraphRegion(currentRegion, currentPlan);
});
const oversizedMessage = computed(() => {
  const currentPlan = plan.value;
  if (decision.value?.allowed !== false || currentPlan === undefined)
    return undefined;
  return `${currentPlan.selectedChunks.toLocaleString()} graph tiles · ${formatBytes(currentPlan.compressedBytes)} planned. Zoom in or open the recommended window.`;
});
const canOpenAnyway = computed(() => {
  const current = model.value;
  return (
    !extendedDisplayBudget.value &&
    current !== undefined &&
    !current.withinDisplayLimits &&
    current.counts.displayedNodeGroups <=
      EXTENDED_TUBE_MAP_DISPLAY_LIMITS.maxDisplayedNodeGroups &&
    current.counts.displayedTopologyEdges <=
      EXTENDED_TUBE_MAP_DISPLAY_LIMITS.maxDisplayedTopologyEdges
  );
});
onMounted(() => {
  document.body.classList.add("pangenome-browser-active");
  window.addEventListener("popstate", onPopState);
  window.addEventListener("keydown", onGlobalKeydown, { capture: true });
  viewport.value = viewportFromUrl();
  void openSource(sourceFromUrl(), true);
});

onBeforeUnmount(() => {
  sourceOperation += 1;
  regionOperation += 1;
  sourceController?.abort();
  regionController?.abort();
  searchController?.abort();
  if (searchTimer !== undefined) clearTimeout(searchTimer);
  if (viewportUrlFrame !== undefined) cancelAnimationFrame(viewportUrlFrame);
  void archive.value?.close();
  window.removeEventListener("popstate", onPopState);
  window.removeEventListener("keydown", onGlobalKeydown, { capture: true });
  document.body.classList.remove("pangenome-browser-active");
});

watch(command, scheduleSearch);
watch(
  options,
  () => {
    rebuildModel();
  },
  { deep: true },
);

async function openSource(
  source: ArchiveSourceSelection,
  restoreUrl = false,
): Promise<void> {
  const operation = ++sourceOperation;
  sourceController?.abort();
  sourceController = new AbortController();
  regionController?.abort();
  const previous = archive.value;
  phase.value = "opening";
  message.value = `Opening ${source.label}`;
  sourceOpen.value = false;
  selection.value = undefined;
  const started = performance.now();
  try {
    const opened = await openPangenome({
      source: source.source,
      signal: sourceController.signal,
      httpUseHead: false,
    });
    const openedInfo = await opened.info({ signal: sourceController.signal });
    if (operation !== sourceOperation) {
      await opened.close();
      return;
    }
    await previous?.close();
    archive.value = opened;
    info.value = openedInfo;
    references.value = opened.references();
    activeSourceLabel.value = source.label;
    activeSourceId.value = source.id;
    activeSourceKey.value = source.key;
    activeCustomUrl.value =
      source.id === "custom" && typeof source.source === "string"
        ? source.source
        : "";
    metrics.value = { openMs: performance.now() - started };
    const restoredLocus = restoreUrl
      ? await locusFromUrl(opened, sourceController.signal)
      : undefined;
    const restored = restoreUrl ? regionFromUrl() : undefined;
    if (restored !== undefined) {
      locus.value = restoredLocus;
      assignCommand(
        restoredLocus?.displayName ??
          formatGenomicCoordinate(
            restored.sample,
            restored.contig,
            restored.start,
            restored.end,
          ),
      );
      await navigate(restored, "replace", true);
      return;
    }
    if (restoredLocus !== undefined) {
      locus.value = restoredLocus;
      assignCommand(restoredLocus.displayName);
      await navigate(paddedLocus(restoredLocus), "replace", true);
      return;
    }
    locus.value = undefined;
    viewport.value = undefined;
    const initial = await initialRegion(opened);
    assignCommand(locus.value?.displayName ?? formatRegion(initial));
    await navigate(initial, "replace");
  } catch (cause) {
    if (operation !== sourceOperation || isAbort(cause)) return;
    fail(cause, "The static archive could not be opened.");
  }
}

async function initialRegion(opened: PangenomeArchive): Promise<RegionQuery> {
  const first = references.value[0];
  if (first === undefined)
    throw new Error("Archive contains no reference paths.");
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
        locus.value = hit;
        return paddedLocus(hit);
      }
    } catch {
      // A missing optional locus index does not make coordinate queries unusable.
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
    end: Math.min(preferred.end, preferred.start + 40_000),
    context: 100,
  };
}

async function navigate(
  nextRegion: RegionQuery,
  historyMode: "push" | "replace" | "none" = "push",
  preserveViewport = false,
): Promise<void> {
  if (!preserveViewport) viewport.value = undefined;
  const bounded = clampRegion(nextRegion);
  region.value = bounded;
  if (historyMode !== "none") updateUrl(historyMode, bounded);
  await loadRegion(bounded);
}

async function loadRegion(nextRegion: RegionQuery): Promise<void> {
  const opened = archive.value;
  if (opened === undefined) return;
  const operation = ++regionOperation;
  regionController?.abort();
  regionController = new AbortController();
  const signal = regionController.signal;
  phase.value = "planning";
  extendedDisplayBudget.value = false;
  message.value = `Planning exact ranges for ${formatShortRegion(nextRegion)}`;
  plan.value = undefined;
  trace.value = undefined;
  metrics.value = { openMs: metrics.value.openMs };
  const started = performance.now();
  try {
    const nextPlan = await opened.planRegion({ ...nextRegion, signal });
    if (operation !== regionOperation) return;
    plan.value = nextPlan;
    void loadSummary(opened, nextRegion, signal, operation);
    if (!decideGraphRegion(nextRegion, nextPlan).allowed) {
      phase.value = "ready";
      message.value = "Interval planned; detailed payloads were not downloaded";
      return;
    }
    phase.value = "streaming";
    message.value = `Streaming 0/${nextPlan.selectedChunks} graph tiles`;
    const nextTiles: RegionTile[] = [];
    let first = true;
    for await (const tile of opened.queryTiles({
      ...nextRegion,
      signal,
      trace: (nextTrace) => {
        if (operation === regionOperation) trace.value = nextTrace;
      },
    })) {
      if (operation !== regionOperation) return;
      nextTiles.push(tile);
      tiles.value = [...nextTiles];
      if (first) {
        first = false;
        metrics.value = {
          ...metrics.value,
          firstTileMs: performance.now() - started,
        };
      }
      rebuildModel();
      message.value = `Streaming ${nextTiles.length}/${nextPlan.selectedChunks} graph tiles`;
    }
    if (operation !== regionOperation) return;
    if (nextTiles.length === 0) {
      tiles.value = [];
      rebuildModel();
    }
    metrics.value = {
      ...metrics.value,
      completeMs: performance.now() - started,
    };
    phase.value = "ready";
    message.value =
      nextTiles.length === 0
        ? "No regional graph tiles overlap this interval"
        : "Local graph ready";
  } catch (cause) {
    if (operation !== regionOperation || isAbort(cause)) return;
    fail(cause, "This genomic interval could not be loaded.");
  }
}

async function loadSummary(
  opened: PangenomeArchive,
  nextRegion: RegionQuery,
  signal: AbortSignal,
  operation: number,
): Promise<void> {
  if (!opened.capabilities().multiscaleSummaries) {
    bins.value = [];
    return;
  }
  try {
    const result = await opened.summary({ ...nextRegion, maxBins: 48, signal });
    if (operation === regionOperation)
      bins.value = result.bins.length >= 4 ? result.bins : [];
  } catch (cause) {
    if (!isAbort(cause) && operation === regionOperation) bins.value = [];
  }
}

function rebuildModel(): void {
  const currentRegion = region.value;
  if (currentRegion === undefined) return;
  const next = buildTubeMapModel(tiles.value, currentRegion, {
    maxPatterns: options.value.patternCount,
    simplifyLinearChains: options.value.simplifyLinearChains,
    expandedNodeGroups: expandedGroups.value,
    ...(extendedDisplayBudget.value ? EXTENDED_TUBE_MAP_DISPLAY_LIMITS : {}),
  });
  model.value = next;
  const selected = selection.value;
  if (selected?.kind === "pattern") {
    const pattern = next.patterns.find(
      (candidate) => candidate.id === selected.pattern.id,
    );
    selection.value =
      pattern === undefined ? undefined : { kind: "pattern", pattern };
  }
}

function scheduleSearch(): void {
  if (searchTimer !== undefined) clearTimeout(searchTimer);
  searchController?.abort();
  suggestions.value = [];
  activeSuggestion.value = 0;
  searchMessage.value = "";
  if (suppressedCommand === command.value) {
    suppressedCommand = undefined;
    return;
  }
  const value = command.value.trim();
  const opened = archive.value;
  if (value.length === 0 || opened === undefined) return;
  try {
    if (
      parseGenomicCommand(value, references.value, region.value?.sample)
        .kind === "coordinate"
    )
      return;
  } catch (cause) {
    searchMessage.value =
      cause instanceof Error ? cause.message : String(cause);
    return;
  }
  if (!opened.capabilities().namedLoci) {
    searchMessage.value =
      "This archive has no named-locus index; enter a coordinate.";
    return;
  }
  searchTimer = setTimeout(() => void searchPrefix(value), 160);
}

async function searchPrefix(value: string): Promise<void> {
  const opened = archive.value;
  if (opened === undefined) return;
  searchController = new AbortController();
  searching.value = true;
  try {
    const result = await opened.searchLoci({
      name: value,
      mode: "prefix",
      sample: region.value?.sample,
      limit: 8,
      signal: searchController.signal,
    });
    suggestions.value = result.hits;
    if (result.hits.length === 0)
      searchMessage.value = "No matching named locus.";
  } catch (cause) {
    if (!isAbort(cause))
      searchMessage.value =
        cause instanceof Error ? cause.message : String(cause);
  } finally {
    searching.value = false;
  }
}

async function submitCommand(): Promise<void> {
  const value = command.value.trim();
  if (value.length === 0) return;
  try {
    const parsed = parseGenomicCommand(
      value,
      references.value,
      region.value?.sample,
    );
    if (parsed.kind === "coordinate") {
      locus.value = undefined;
      suggestions.value = [];
      await navigate({
        ...parsed.reference,
        start: parsed.start,
        end: parsed.end,
        context: 100,
      });
      return;
    }
    const opened = archive.value;
    if (opened === undefined) return;
    const result = await opened.searchLoci({
      name: parsed.name,
      mode: "exact",
      sample: region.value?.sample,
      limit: 1,
    });
    const hit = result.hits[0];
    if (hit === undefined) {
      searchMessage.value = `No exact locus named ${parsed.name}.`;
      return;
    }
    await selectLocus(hit);
  } catch (cause) {
    searchMessage.value =
      cause instanceof Error ? cause.message : String(cause);
  }
}

async function selectLocus(hit: LocusHit): Promise<void> {
  locus.value = hit;
  assignCommand(hit.displayName);
  suggestions.value = [];
  searchMessage.value = "";
  await navigate(paddedLocus(hit));
}

function paddedLocus(hit: LocusHit): RegionQuery {
  const reference = referenceFor(hit.reference);
  return {
    sample: hit.reference.sample,
    contig: hit.reference.contig,
    start: Math.max(
      reference?.start ?? 0,
      hit.reference.start - configuredPadding,
    ),
    end: Math.min(
      reference?.end ?? hit.reference.end + configuredPadding,
      hit.reference.end + configuredPadding,
    ),
    context: 100,
  };
}

function openRecommended(): void {
  const current = region.value;
  const reference = current === undefined ? undefined : referenceFor(current);
  if (current === undefined || reference === undefined) return;
  void navigate(recommendedGraphRegion(current, reference, locus.value));
}

function openAnyway(): void {
  extendedDisplayBudget.value = true;
  rebuildModel();
}

function expandGroup(key: string): void {
  if (!expandedGroups.value.includes(key))
    expandedGroups.value = [...expandedGroups.value, key];
  selection.value = undefined;
  rebuildModel();
}

function updateMetrics(next: { layoutMs: number; svgElements: number }): void {
  metrics.value = { ...metrics.value, ...next };
}

function updateOptions(next: GraphOptions): void {
  if (next.simplifyLinearChains !== options.value.simplifyLinearChains)
    extendedDisplayBudget.value = false;
  options.value = next;
}

function updateViewport(next: GraphViewport): void {
  viewport.value = next;
  if (viewportUrlFrame !== undefined) return;
  viewportUrlFrame = requestAnimationFrame(() => {
    viewportUrlFrame = undefined;
    const current = region.value;
    if (current !== undefined) updateUrl("replace", current);
  });
}

function closeSearch(): void {
  suggestions.value = [];
  searchMessage.value = "";
}

function goBack(): void {
  window.history.back();
}

function goForward(): void {
  window.history.forward();
}

function onPopState(): void {
  const source = sourceFromUrl();
  viewport.value = viewportFromUrl();
  if (source.key !== activeSourceKey.value) {
    void openSource(source, true);
    return;
  }
  const opened = archive.value;
  if (opened === undefined) return;
  const restored = regionFromUrl();
  if (restored === undefined) return;
  phase.value = "planning";
  message.value = `Restoring ${formatShortRegion(restored)} from browser history`;
  void (async () => {
    const restoredLocus = await locusFromUrl(opened);
    locus.value = restoredLocus;
    assignCommand(restoredLocus?.displayName ?? formatRegion(restored));
    await navigate(restored, "none", true);
  })();
}

function onGlobalKeydown(event: KeyboardEvent): void {
  const target = event.target as HTMLElement | null;
  if (event.key === "Escape") {
    selection.value = undefined;
    sourceOpen.value = false;
    shareOpen.value = false;
    closeSearch();
  } else if (
    event.key === "Home" &&
    target?.tagName !== "INPUT" &&
    target?.tagName !== "TEXTAREA"
  ) {
    event.preventDefault();
    tubeMap.value?.fit();
  } else if (
    (event.metaKey || event.ctrlKey) &&
    event.key.toLowerCase() === "k"
  ) {
    event.preventDefault();
    event.stopImmediatePropagation();
    focusLocationSearch();
  } else if (
    event.key === "/" &&
    target?.tagName !== "INPUT" &&
    target?.tagName !== "TEXTAREA"
  ) {
    event.preventDefault();
    event.stopImmediatePropagation();
    focusLocationSearch();
  }
}

function focusLocationSearch(): void {
  queueMicrotask(() => {
    root.value
      ?.querySelector<HTMLInputElement>(
        '[aria-label="Search gene or coordinate"]',
      )
      ?.focus();
  });
}

function share(): void {
  sourceOpen.value = false;
  shareUrl.value = window.location.href;
  shareOpen.value = true;
}

async function copy(value: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(value);
    message.value = "Sequence copied";
  } catch {
    message.value = "Clipboard access was unavailable";
  }
}

function updateUrl(mode: "push" | "replace", nextRegion: RegionQuery): void {
  const url = new URL(window.location.href);
  url.searchParams.set("archive", activeSourceId.value);
  if (activeSourceId.value === "custom" && activeCustomUrl.value.length > 0)
    url.searchParams.set("url", activeCustomUrl.value);
  else url.searchParams.delete("url");
  url.searchParams.set("sample", nextRegion.sample);
  url.searchParams.set("contig", nextRegion.contig);
  url.searchParams.set("start", String(nextRegion.start));
  url.searchParams.set("end", String(nextRegion.end));
  if (locus.value === undefined) url.searchParams.delete("locus");
  else url.searchParams.set("locus", locus.value.displayName);
  writeViewport(url, viewport.value);
  window.history[mode === "push" ? "pushState" : "replaceState"](null, "", url);
}

function regionFromUrl(): RegionQuery | undefined {
  const parameters = new URLSearchParams(window.location.search);
  const sample = parameters.get("sample");
  const contig = parameters.get("contig");
  const start = Number(parameters.get("start"));
  const end = Number(parameters.get("end"));
  if (
    sample === null ||
    contig === null ||
    !Number.isSafeInteger(start) ||
    !Number.isSafeInteger(end) ||
    end <= start
  )
    return undefined;
  const candidate = { sample, contig, start, end, context: 100 };
  return referenceFor(candidate) === undefined
    ? undefined
    : clampRegion(candidate);
}

function sourceFromUrl(): ArchiveSourceSelection {
  const parameters = new URLSearchParams(window.location.search);
  const requested = parameters.get("archive");
  const canonicalId =
    requested === "population"
      ? "1000g"
      : requested === "configured"
        ? "hprc"
        : requested;
  const preset = demoSources.value.find((source) => source.id === canonicalId);
  if (preset !== undefined) return preset;
  if (canonicalId === "custom") {
    const custom = parameters.get("url")?.trim() ?? "";
    if (custom.length > 0) {
      const absolute = new URL(custom, window.location.href).href;
      return {
        id: "custom",
        source: absolute,
        label: "Custom remote archive",
        key: `url:${absolute}`,
      };
    }
  }
  const fallback =
    demoSources.value.find((source) => source.id === "hprc") ??
    demoSources.value.find((source) => source.id === "1000g") ??
    demoSources.value[0];
  if (fallback === undefined)
    throw new Error("No demo archive source is available.");
  return fallback;
}

async function locusFromUrl(
  opened: PangenomeArchive,
  signal?: AbortSignal,
): Promise<LocusHit | undefined> {
  const name = new URLSearchParams(window.location.search).get("locus")?.trim();
  if (!name || !opened.capabilities().namedLoci) return undefined;
  try {
    const result = await opened.searchLoci({
      name,
      mode: "exact",
      sample:
        new URLSearchParams(window.location.search).get("sample") ?? undefined,
      limit: 1,
      signal,
    });
    return result.hits[0];
  } catch {
    return undefined;
  }
}

function viewportFromUrl(): GraphViewport | undefined {
  const parameters = new URLSearchParams(window.location.search);
  const zoom = Number(parameters.get("zoom"));
  const center = Number(parameters.get("center"));
  const verticalScale = Number(parameters.get("vscale"));
  if (
    !Number.isFinite(zoom) ||
    zoom < 0.2 ||
    zoom > 5 ||
    !Number.isFinite(center) ||
    center < -1 ||
    center > 2 ||
    !Number.isFinite(verticalScale) ||
    verticalScale < 0.75 ||
    verticalScale > 1.45
  )
    return undefined;
  return { zoom, center, verticalScale };
}

function writeViewport(url: URL, value: GraphViewport | undefined): void {
  if (value === undefined) {
    url.searchParams.delete("zoom");
    url.searchParams.delete("center");
    url.searchParams.delete("vscale");
    return;
  }
  url.searchParams.set("zoom", formatViewportNumber(value.zoom, 4));
  url.searchParams.set("center", formatViewportNumber(value.center, 6));
  url.searchParams.set("vscale", formatViewportNumber(value.verticalScale, 4));
}

function formatViewportNumber(value: number, digits: number): string {
  return value.toFixed(digits).replace(/(?:\.0+|(\.\d*?)0+)$/, "$1");
}

function clampRegion(candidate: RegionQuery): RegionQuery {
  const reference = referenceFor(candidate);
  if (reference === undefined) return candidate;
  const start = Math.max(reference.start, candidate.start);
  const end = Math.min(reference.end, Math.max(start + 1, candidate.end));
  return { ...candidate, start, end };
}

function referenceFor(
  candidate: Pick<RegionQuery, "sample" | "contig">,
): ReferenceDescriptor | undefined {
  return references.value.find(
    (reference) =>
      reference.sample === candidate.sample &&
      reference.contig === candidate.contig,
  );
}

function formatRegion(value: RegionQuery): string {
  return formatGenomicCoordinate(
    value.sample,
    value.contig,
    value.start,
    value.end,
  );
}

function assignCommand(value: string): void {
  suppressedCommand = value;
  command.value = value;
}

function formatShortRegion(value: RegionQuery): string {
  return `${value.contig}:${value.start.toLocaleString()}–${value.end.toLocaleString()}`;
}

function formatBytes(value: bigint): string {
  const bytes = Number(value);
  return bytes >= 1024 ** 2
    ? `${(bytes / 1024 ** 2).toFixed(1)} MiB`
    : `${(bytes / 1024).toFixed(1)} KiB`;
}

function isAbort(cause: unknown): boolean {
  return cause instanceof DOMException && cause.name === "AbortError";
}

function fail(cause: unknown, prefix: string): void {
  phase.value = "error";
  message.value = `${prefix} ${cause instanceof Error ? cause.message : String(cause)}`;
}
</script>

<template>
  <div ref="root" class="pangenome-browser">
    <BrowserToolbar
      :command="command"
      :suggestions="suggestions"
      :active-suggestion="activeSuggestion"
      :searching="searching"
      :disabled="phase === 'opening'"
      :search-message="searchMessage || undefined"
      :options="options"
      @update:command="command = $event"
      @update:active-suggestion="activeSuggestion = $event"
      @update:options="updateOptions"
      @submit="submitCommand"
      @select="selectLocus"
      @close-search="closeSearch"
      @back="goBack"
      @forward="goForward"
      @zoom-out="tubeMap?.zoomOut()"
      @zoom-in="tubeMap?.zoomIn()"
      @fit="tubeMap?.fit()"
      @vertical-out="tubeMap?.decreaseVerticalSpacing()"
      @vertical-in="tubeMap?.increaseVerticalSpacing()"
      @archive="shareOpen = false; sourceOpen = !sourceOpen"
      @share="share"
    />
    <LinearReferenceTrack :region="region" :locus="locus" :plan="plan" :bins="bins" />
    <div class="browser-graph-shell">
      <TubeMapView
        ref="tubeMap"
        :model="model"
        :phase="phase"
        :message="message"
        :oversized-message="oversizedMessage"
        :can-open-anyway="canOpenAnyway"
        :options="options"
        :selection="selection"
        :viewport="viewport"
        @select="selection = $event"
        @metrics="updateMetrics"
        @recommended="openRecommended"
        @open="openAnyway"
        @viewport="updateViewport"
      />
      <NodeInspector v-if="selection?.kind === 'node'" :node="selection.node" :model="model" @close="selection = undefined" @copy="copy" @expand="expandGroup" />
      <PatternInspector v-else-if="selection?.kind === 'pattern'" :pattern="selection.pattern" @close="selection = undefined" />
      <ArchiveSourceMenu
        :open="sourceOpen"
        :presets="demoSources"
        :active-id="activeSourceId"
        :active-label="activeSourceLabel"
        :info="info"
        @close="sourceOpen = false"
        @select="openSource"
      />
    </div>
    <BrowserStatusBar
      :phase="phase"
      :message="message"
      :info="info"
      :plan="plan"
      :trace="trace"
      :model="model"
      :metrics="metrics"
    />
    <ShareDialog :open="shareOpen" :url="shareUrl" @close="shareOpen = false" />
  </div>
</template>
