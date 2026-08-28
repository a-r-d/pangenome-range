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
// biome-ignore lint/style/useImportType: the Vue template needs the runtime component.
import TubeMapView from "./TubeMapView.vue";
import type {
  ArchiveSourceSelection,
  BrowserMetrics,
  BrowserPhase,
  BrowserSelection,
  GraphOptions,
} from "./types";
import "./browser.css";

const configuredArchiveUrl =
  (
    import.meta.env.VITE_PANGENOME_RANGE_DEMO_ARCHIVE_URL as string | undefined
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
const activeSourceLabel = ref("Configured HPRC archive");
const expandedGroups = ref<readonly string[]>([]);
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
const configuredLabel = "Configured HPRC v2.1 + GENCODE v50";

onMounted(() => {
  document.body.classList.add("pangenome-browser-active");
  window.addEventListener("popstate", onPopState);
  window.addEventListener("keydown", onGlobalKeydown, { capture: true });
  const fixtureRequested =
    new URLSearchParams(window.location.search).get("archive") === "fixture";
  const initialSource: ArchiveSourceSelection =
    configuredArchiveUrl.length > 0 && !fixtureRequested
      ? {
          source: configuredArchiveUrl,
          label: configuredLabel,
          key: `url:${configuredArchiveUrl}`,
        }
      : {
          source: fixtureUrl,
          label: "Bundled deterministic fixture",
          key: `url:${fixtureUrl}`,
        };
  void openSource(initialSource);
});

onBeforeUnmount(() => {
  sourceOperation += 1;
  regionOperation += 1;
  sourceController?.abort();
  regionController?.abort();
  searchController?.abort();
  if (searchTimer !== undefined) clearTimeout(searchTimer);
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

async function openSource(source: ArchiveSourceSelection): Promise<void> {
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
    metrics.value = { openMs: performance.now() - started };
    const restored = previous === undefined ? regionFromUrl() : undefined;
    if (restored !== undefined) {
      locus.value = undefined;
      assignCommand(
        formatGenomicCoordinate(
          restored.sample,
          restored.contig,
          restored.start,
          restored.end,
        ),
      );
      await navigate(restored, "replace");
      return;
    }
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
): Promise<void> {
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
  options.value = next;
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
  const restored = regionFromUrl();
  if (restored !== undefined) {
    locus.value = undefined;
    assignCommand(formatRegion(restored));
    void navigate(restored, "none");
  }
}

function onGlobalKeydown(event: KeyboardEvent): void {
  const target = event.target as HTMLElement | null;
  if (event.key === "Escape") {
    selection.value = undefined;
    sourceOpen.value = false;
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

async function share(): Promise<void> {
  try {
    await navigator.clipboard.writeText(window.location.href);
    message.value = "Shareable region URL copied";
  } catch {
    message.value = "Copy the current URL to share this exact region";
  }
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
  url.searchParams.set("sample", nextRegion.sample);
  url.searchParams.set("contig", nextRegion.contig);
  url.searchParams.set("start", String(nextRegion.start));
  url.searchParams.set("end", String(nextRegion.end));
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
  return clampRegion({ sample, contig, start, end, context: 100 });
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
      @archive="sourceOpen = !sourceOpen"
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
        :options="options"
        :selection="selection"
        @select="selection = $event"
        @metrics="updateMetrics"
        @recommended="openRecommended"
      />
      <NodeInspector v-if="selection?.kind === 'node'" :node="selection.node" :model="model" @close="selection = undefined" @copy="copy" @expand="expandGroup" />
      <PatternInspector v-else-if="selection?.kind === 'pattern'" :pattern="selection.pattern" @close="selection = undefined" />
      <ArchiveSourceMenu
        :open="sourceOpen"
        :configured-url="configuredArchiveUrl"
        :configured-label="configuredLabel"
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
  </div>
</template>
