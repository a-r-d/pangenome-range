<script setup lang="ts">
// biome-ignore-all lint/correctness/noUnusedVariables: Vue template bindings are consumed outside the script AST.

import {
  type ArchiveCacheStats,
  openPangenome,
  type PangenomeArchive,
  type QueryTrace,
  type ReferenceDescriptor,
  type RegionQuery,
} from "pangenome-range/reader";
import {
  createPangenomeViewer,
  type PangenomeViewer,
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
} from "vue";

type ArchiveChoice = "fixture" | "configured" | "custom" | "local";
type Phase = "idle" | "opening" | "querying" | "ready" | "error";

const configuredArchiveUrl =
  (
    import.meta.env.VITE_PANGENOME_RANGE_DEMO_ARCHIVE_URL as string | undefined
  )?.trim() ?? "";
const viewerHost = ref<HTMLElement>();
const archiveChoice = ref<ArchiveChoice>("fixture");
const customUrl = ref("");
const localFile = shallowRef<File>();
const archive = shallowRef<PangenomeArchive>();
const viewer = shallowRef<PangenomeViewer>();
const references = ref<readonly ReferenceDescriptor[]>([]);
const sample = ref("GRCh38");
const contig = ref("chr1");
const start = ref(100);
const end = ref(102);
const context = ref(100);
const phase = ref<Phase>("idle");
const statusMessage = ref("Preparing the bundled deterministic fixture…");
const errorMessage = ref("");
const shareMessage = ref("");
const activeArchiveLabel = ref("Not opened");
const activeSourceKey = ref("");
const progress = shallowRef<ViewerProgress>();
const queryTrace = shallowRef<QueryTrace>();
const cacheStats = shallowRef<ArchiveCacheStats>();
const openMs = ref<number>();
const queryMs = ref<number>();
const totalMs = ref<number>();
let operation = 0;
let loadController: AbortController | undefined;
let unsubscribeViewer: (() => void)[] = [];

const samples = computed(() => [
  ...new Set(references.value.map((reference) => reference.sample)),
]);
const contigs = computed(() => [
  ...new Set(
    references.value
      .filter((reference) => reference.sample === sample.value)
      .map((reference) => reference.contig),
  ),
]);
const activeReference = computed(() =>
  references.value.find(
    (reference) =>
      reference.sample === sample.value && reference.contig === contig.value,
  ),
);
const isLoading = computed(
  () => phase.value === "opening" || phase.value === "querying",
);
const semantics = computed(
  () => archive.value?.semantics ?? "anonymous-distinct-weighted-tile-paths",
);

onMounted(async () => {
  restoreUrlState();
  await nextTick();
  await load().catch(() => undefined);
});

onBeforeUnmount(() => {
  operation += 1;
  loadController?.abort();
  detachViewer();
  void archive.value?.close();
  archive.value = undefined;
});

async function load(): Promise<void> {
  const currentOperation = ++operation;
  errorMessage.value = "";
  shareMessage.value = "";
  const startedAt = performance.now();
  try {
    const source = selectedSource();
    if (archive.value === undefined || activeSourceKey.value !== source.key) {
      await openArchive(source, currentOperation);
    } else {
      openMs.value = 0;
    }
    if (currentOperation !== operation || archive.value === undefined) return;
    const region = selectedRegion();
    phase.value = "querying";
    statusMessage.value = `Streaming ${region.sample} ${region.contig}:${region.start}-${region.end}`;
    progress.value = undefined;
    queryTrace.value = undefined;
    const queryStartedAt = performance.now();
    await viewer.value?.setRegion(region);
    if (currentOperation !== operation) return;
    queryMs.value = performance.now() - queryStartedAt;
    totalMs.value = performance.now() - startedAt;
    cacheStats.value = archive.value.cacheStats();
    phase.value = "ready";
    statusMessage.value =
      "Region ready — scroll or use the buttons to zoom; drag to pan.";
    updateUrlState();
  } catch (cause) {
    if (currentOperation !== operation || isAbort(cause)) return;
    phase.value = "error";
    errorMessage.value = actionableError(cause);
    statusMessage.value = "The archive or region could not be loaded.";
    throw cause;
  }
}

async function openArchive(
  source: { source: string | Blob; key: string; label: string },
  currentOperation: number,
): Promise<void> {
  loadController?.abort();
  loadController = new AbortController();
  detachViewer();
  await archive.value?.close();
  archive.value = undefined;
  activeSourceKey.value = "";
  phase.value = "opening";
  statusMessage.value = `Opening ${source.label}`;
  const startedAt = performance.now();
  const opened = await openPangenome({
    source: source.source,
    signal: loadController.signal,
    httpUseHead: false,
  });
  if (currentOperation !== operation) {
    await opened.close();
    return;
  }
  openMs.value = performance.now() - startedAt;
  archive.value = opened;
  activeSourceKey.value = source.key;
  activeArchiveLabel.value = source.label;
  references.value = opened.references();
  selectAvailableReference();
  await nextTick();
  if (viewerHost.value === undefined) {
    throw new Error("viewer host did not mount");
  }
  const nextViewer = createPangenomeViewer(viewerHost.value, {
    archive: opened,
    maxRenderedNodes: 2_000,
    maxRenderedEdges: 4_000,
    maxHaplotypeLanes: 24,
    showRequestTrace: true,
  });
  viewer.value = nextViewer;
  unsubscribeViewer = [
    nextViewer.on("progress", (detail) => {
      progress.value = detail;
      statusMessage.value = `Decoded ${detail.counts.tiles} tile${detail.counts.tiles === 1 ? "" : "s"}`;
    }),
    nextViewer.on("querytrace", (trace) => {
      queryTrace.value = trace;
      cacheStats.value = opened.cacheStats();
    }),
    nextViewer.on("error", ({ error }) => {
      if (!isAbort(error)) errorMessage.value = actionableError(error);
    }),
  ];
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
  let url: string;
  let label: string;
  if (archiveChoice.value === "fixture") {
    url = new URL(withBase("/fixtures/format-v1.pngr"), window.location.href)
      .href;
    label = "deterministic synthetic fixture";
  } else if (archiveChoice.value === "configured") {
    if (configuredArchiveUrl.length === 0) {
      throw new Error(
        "No external archive is configured. Set VITE_PANGENOME_RANGE_DEMO_ARCHIVE_URL when building the site.",
      );
    }
    url = new URL(configuredArchiveUrl, window.location.href).href;
    label = "configured immutable archive";
  } else {
    if (customUrl.value.trim().length === 0)
      throw new Error("Enter an archive URL.");
    url = new URL(customUrl.value.trim(), window.location.href).href;
    label = "custom remote archive";
  }
  return { source: url, key: `url:${url}`, label };
}

function selectedRegion(): RegionQuery {
  if (!Number.isSafeInteger(start.value) || !Number.isSafeInteger(end.value)) {
    throw new Error("Start and end must be safe integer coordinates.");
  }
  if (end.value <= start.value)
    throw new Error("End must be greater than start.");
  if (
    !Number.isSafeInteger(context.value) ||
    context.value < 0 ||
    context.value > 100
  ) {
    throw new Error("Context must be an integer from 0 through 100.");
  }
  return {
    sample: sample.value,
    contig: contig.value,
    start: start.value,
    end: end.value,
    context: context.value,
  };
}

function selectAvailableReference(): void {
  if (activeReference.value !== undefined) return;
  const first = references.value[0];
  if (first === undefined)
    throw new Error("Archive contains no reference descriptors.");
  sample.value = first.sample;
  contig.value = first.contig;
  applyReferenceBounds(first);
}

function onSampleChange(): void {
  const first = references.value.find(
    (reference) => reference.sample === sample.value,
  );
  if (first === undefined) return;
  contig.value = first.contig;
  applyReferenceBounds(first);
}

function onContigChange(): void {
  if (activeReference.value !== undefined)
    applyReferenceBounds(activeReference.value);
}

function onPresetChange(event: Event): void {
  const index = Number((event.currentTarget as HTMLSelectElement).value);
  const reference = references.value[index];
  if (reference === undefined) return;
  sample.value = reference.sample;
  contig.value = reference.contig;
  applyReferenceBounds(reference);
}

function applyReferenceBounds(reference: ReferenceDescriptor): void {
  start.value = reference.start;
  end.value = Math.min(reference.end, reference.start + 100_000);
}

function onLocalFile(event: Event): void {
  const input = event.currentTarget as HTMLInputElement;
  localFile.value = input.files?.[0];
  if (localFile.value !== undefined) archiveChoice.value = "local";
}

function detachViewer(): void {
  for (const unsubscribe of unsubscribeViewer) unsubscribe();
  unsubscribeViewer = [];
  viewer.value?.destroy();
  viewer.value = undefined;
}

function restoreUrlState(): void {
  const params = new URLSearchParams(window.location.search);
  const choice = params.get("archive");
  if (choice === "configured" && configuredArchiveUrl.length > 0) {
    archiveChoice.value = "configured";
  } else if (choice === "custom" && params.has("url")) {
    archiveChoice.value = "custom";
    customUrl.value = params.get("url") ?? "";
  }
  sample.value = params.get("sample") ?? sample.value;
  contig.value = params.get("contig") ?? contig.value;
  start.value = safeUrlInteger(params.get("start"), start.value);
  end.value = safeUrlInteger(params.get("end"), end.value);
  context.value = safeUrlInteger(params.get("context"), context.value);
}

function updateUrlState(): void {
  if (archiveChoice.value === "local") return;
  const params = new URLSearchParams();
  params.set("archive", archiveChoice.value);
  if (archiveChoice.value === "custom")
    params.set("url", customUrl.value.trim());
  params.set("sample", sample.value);
  params.set("contig", contig.value);
  params.set("start", String(start.value));
  params.set("end", String(end.value));
  params.set("context", String(context.value));
  window.history.replaceState(
    null,
    "",
    `${window.location.pathname}?${params}`,
  );
}

async function copyShareLink(): Promise<void> {
  if (archiveChoice.value === "local") {
    shareMessage.value = "Local files cannot be embedded in a shareable URL.";
    return;
  }
  updateUrlState();
  try {
    await navigator.clipboard.writeText(window.location.href);
    shareMessage.value = "Shareable region URL copied.";
  } catch {
    shareMessage.value =
      "The URL now contains this region; copy it from the address bar.";
  }
}

function safeUrlInteger(value: string | null, fallback: number): number {
  if (value === null || !/^\d+$/.test(value)) return fallback;
  const number = Number(value);
  return Number.isSafeInteger(number) ? number : fallback;
}

function actionableError(cause: unknown): string {
  const message = cause instanceof Error ? cause.message : String(cause);
  if (/failed to fetch|cors|networkerror/i.test(message)) {
    return `${message} The remote origin must allow this Pages origin with CORS and expose range headers; see Hosting an archive below.`;
  }
  if (
    /206|content-range|range request|returned 200|full response/i.test(message)
  ) {
    return `${message} The origin must honor byte ranges with 206 Partial Content and an exact Content-Range; the reader will not silently download a large object.`;
  }
  return message;
}

function isAbort(cause: unknown): boolean {
  return cause instanceof DOMException && cause.name === "AbortError";
}

function formatBytes(bytes: number | undefined): string {
  if (bytes === undefined) return "—";
  if (bytes < 1_024) return `${bytes} B`;
  if (bytes < 1_048_576) return `${(bytes / 1_024).toFixed(1)} KiB`;
  return `${(bytes / 1_048_576).toFixed(1)} MiB`;
}

function formatMs(milliseconds: number | undefined): string {
  return milliseconds === undefined ? "—" : `${milliseconds.toFixed(1)} ms`;
}
</script>

<template>
  <section class="demo-shell" aria-labelledby="demo-title">
    <header class="demo-hero">
      <div>
        <p class="eyebrow">Static object · real byte ranges</p>
        <h1 id="demo-title">Regional pangenome explorer</h1>
        <p>
          Open a remote immutable <code>.pngr</code> object or a local file, then stream
          and render one reference interval without a query server.
        </p>
      </div>
      <div class="phase" :data-phase="phase">
        <span aria-hidden="true"></span>{{ phase }}
      </div>
    </header>

    <form class="control-panel" @submit.prevent="load">
      <label>
        <span>Archive</span>
        <select v-model="archiveChoice" aria-label="Archive source">
          <option value="fixture">Bundled synthetic fixture</option>
          <option value="configured" :disabled="configuredArchiveUrl.length === 0">
            {{ configuredArchiveUrl.length === 0 ? "External archive (not configured)" : "Configured external archive" }}
          </option>
          <option value="custom">Custom remote URL</option>
          <option value="local">Local .pngr file</option>
        </select>
      </label>
      <label v-if="archiveChoice === 'custom'" class="wide">
        <span>Remote archive URL</span>
        <input v-model="customUrl" type="url" placeholder="https://archive.example/data.pngr" />
      </label>
      <label :class="{ wide: archiveChoice !== 'custom' }">
        <span>Local file</span>
        <input type="file" accept=".pngr,application/octet-stream" @change="onLocalFile" />
      </label>

      <label>
        <span>Reference / sample</span>
        <select v-model="sample" :disabled="samples.length === 0" @change="onSampleChange">
          <option v-for="item in samples" :key="item" :value="item">{{ item }}</option>
        </select>
      </label>
      <label>
        <span>Contig</span>
        <select v-model="contig" :disabled="contigs.length === 0" @change="onContigChange">
          <option v-for="item in contigs" :key="item" :value="item">{{ item }}</option>
        </select>
      </label>
      <label>
        <span>Start</span>
        <input v-model.number="start" type="number" min="0" step="1" />
      </label>
      <label>
        <span>End</span>
        <input v-model.number="end" type="number" min="1" step="1" />
      </label>
      <label>
        <span>Context (0–100 bp)</span>
        <input v-model.number="context" type="number" min="0" max="100" step="1" />
      </label>
      <label>
        <span>Preset region</span>
        <select aria-label="Preset region" :disabled="references.length === 0" @change="onPresetChange">
          <option value="">Choose a reference interval…</option>
          <option v-for="(reference, index) in references" :key="`${reference.sample}:${reference.contig}:${reference.start}`" :value="index">
            {{ reference.sample }} · {{ reference.contig }} · {{ reference.start }}–{{ Math.min(reference.end, reference.start + 100_000) }}
          </option>
        </select>
      </label>
      <div class="actions wide">
        <button class="primary" type="submit">
          {{ isLoading ? "Load selection (cancel current)" : "Load region" }}
        </button>
        <button type="button" :disabled="phase !== 'ready'" @click="copyShareLink">
          Copy region link
        </button>
        <span class="action-note">{{ shareMessage || statusMessage }}</span>
      </div>
    </form>

    <div v-if="errorMessage" class="error-panel" role="alert">
      <strong>Range load failed.</strong> {{ errorMessage }}
      <a :href="withBase('/HOSTING')">Check the required origin headers.</a>
    </div>

    <section class="viewer-panel" aria-label="Pangenome graph visualization">
      <div class="viewer-toolbar">
        <div>
          <strong>{{ activeArchiveLabel }}</strong>
          <span>format v{{ archive?.formatVersion ?? "—" }}</span>
        </div>
        <div class="view-actions" aria-label="View controls">
          <button type="button" aria-label="Pan left" @click="viewer?.panBy(60)">←</button>
          <button type="button" aria-label="Pan right" @click="viewer?.panBy(-60)">→</button>
          <button type="button" aria-label="Zoom out" @click="viewer?.zoomBy(0.8)">−</button>
          <button type="button" aria-label="Zoom in" @click="viewer?.zoomBy(1.25)">+</button>
          <button type="button" @click="viewer?.resetView()">Reset view</button>
        </div>
      </div>
      <div ref="viewerHost" class="viewer-host"></div>
      <p class="semantics-note">
        <strong>Semantics:</strong> {{ semantics }}. Weighted anonymous traversals are
        local evidence within each source tile. They are not named people or globally
        stitchable samples.
      </p>
    </section>

    <section class="metrics-grid" aria-label="Query evidence">
      <article>
        <h2>Decoded view</h2>
        <dl>
          <div><dt>Tiles</dt><dd>{{ progress?.counts.tiles ?? 0 }}</dd></div>
          <div><dt>Nodes</dt><dd>{{ progress?.counts.renderedNodes ?? 0 }} / {{ progress?.counts.decodedNodes ?? 0 }}</dd></div>
          <div><dt>Edges</dt><dd>{{ progress?.counts.renderedEdges ?? 0 }} / {{ progress?.counts.decodedEdges ?? 0 }}</dd></div>
          <div><dt>Local traversals</dt><dd>{{ progress?.counts.renderedHaplotypeLanes ?? 0 }} / {{ progress?.counts.decodedTraversals ?? 0 }}</dd></div>
        </dl>
        <p v-if="progress?.summary" class="budget-note">{{ progress.summary }}</p>
      </article>
      <article>
        <h2>Range I/O</h2>
        <dl>
          <div><dt>Requests</dt><dd>{{ queryTrace?.requestRanges.length ?? 0 }}</dd></div>
          <div><dt>Total bytes</dt><dd>{{ formatBytes(queryTrace?.totalBytes) }}</dd></div>
          <div><dt>Unique bytes</dt><dd>{{ formatBytes(queryTrace?.uniqueBytes) }}</dd></div>
          <div><dt>Dependency rounds</dt><dd>{{ queryTrace?.dependencyRounds ?? "—" }}</dd></div>
        </dl>
      </article>
      <article>
        <h2>Timing</h2>
        <dl>
          <div><dt>Open</dt><dd>{{ formatMs(openMs) }}</dd></div>
          <div><dt>Query wall</dt><dd>{{ formatMs(queryMs) }}</dd></div>
          <div><dt>Decompress</dt><dd>{{ formatMs(queryTrace?.decompressionMs) }}</dd></div>
          <div><dt>Decode</dt><dd>{{ formatMs(queryTrace?.decodeMs) }}</dd></div>
          <div><dt>Total action</dt><dd>{{ formatMs(totalMs) }}</dd></div>
        </dl>
      </article>
      <article>
        <h2>Library cache</h2>
        <dl>
          <div><dt>Directory entries</dt><dd>{{ cacheStats?.directoryEntries ?? 0 }}</dd></div>
          <div><dt>Directory bytes</dt><dd>{{ formatBytes(cacheStats?.directoryBytes) }}</dd></div>
          <div><dt>Payload entries</dt><dd>{{ cacheStats?.payloadEntries ?? 0 }}</dd></div>
          <div><dt>Payload bytes</dt><dd>{{ formatBytes(cacheStats?.payloadBytes) }}</dd></div>
        </dl>
      </article>
    </section>

    <details class="trace-panel" :open="Boolean(queryTrace)">
      <summary>Exact request trace and correctness hash</summary>
      <p v-if="queryTrace" class="hash"><strong>Canonical hash:</strong> {{ queryTrace.canonicalHash }}</p>
      <div class="trace-table-wrap">
        <table>
          <thead><tr><th>Layer</th><th>Offset</th><th>Length</th></tr></thead>
          <tbody>
            <tr v-for="(range, index) in queryTrace?.requestRanges ?? []" :key="`${range.offset}:${index}`">
              <td>{{ range.layer }}</td><td>{{ range.offset.toString() }}</td><td>{{ formatBytes(range.length) }}</td>
            </tr>
          </tbody>
        </table>
      </div>
    </details>
  </section>
</template>

<style scoped>
.demo-shell {
  --demo-ink: #14213d;
  --demo-muted: #5e6f84;
  --demo-teal: #0c7c86;
  --demo-teal-dark: #075c66;
  --demo-coral: #c65d34;
  --demo-paper: #f7fafb;
  --demo-line: #d9e2e8;
  color: var(--demo-ink);
  max-width: 1440px;
  margin: 0 auto 5rem;
}
.demo-hero {
  display: flex;
  justify-content: space-between;
  gap: 2rem;
  align-items: flex-start;
  padding: 1.5rem 0 1.1rem;
}
.demo-hero h1 { margin: 0.15rem 0 0.5rem; font-size: clamp(2rem, 4vw, 3.7rem); line-height: 1; letter-spacing: -0.04em; }
.demo-hero p:not(.eyebrow) { max-width: 760px; margin: 0; color: var(--demo-muted); font-size: 1.05rem; }
.eyebrow { margin: 0; color: var(--demo-coral); font: 700 0.72rem/1.4 ui-monospace, monospace; letter-spacing: 0.14em; text-transform: uppercase; }
.phase { display: flex; gap: 0.55rem; align-items: center; padding: 0.55rem 0.85rem; border: 1px solid var(--demo-line); border-radius: 999px; color: var(--demo-muted); font: 700 0.72rem/1 ui-monospace, monospace; text-transform: uppercase; letter-spacing: 0.08em; white-space: nowrap; }
.phase span { width: 0.55rem; height: 0.55rem; border-radius: 50%; background: #94a3b8; }
.phase[data-phase="ready"] span { background: #22a06b; box-shadow: 0 0 0 4px rgba(34, 160, 107, 0.14); }
.phase[data-phase="opening"] span, .phase[data-phase="querying"] span { background: #f2b134; animation: pulse 1s ease-in-out infinite; }
.phase[data-phase="error"] span { background: #c33d3d; }
@keyframes pulse { 50% { opacity: 0.35; } }
.control-panel { display: grid; grid-template-columns: repeat(5, minmax(130px, 1fr)); gap: 0.9rem; padding: 1.1rem; border: 1px solid var(--demo-line); border-radius: 16px; background: color-mix(in srgb, var(--demo-paper) 94%, transparent); box-shadow: 0 10px 32px rgba(20, 33, 61, 0.06); }
.control-panel label { display: flex; min-width: 0; flex-direction: column; gap: 0.35rem; }
.control-panel label > span { color: var(--demo-muted); font: 700 0.68rem/1.2 ui-monospace, monospace; letter-spacing: 0.07em; text-transform: uppercase; }
.control-panel input, .control-panel select { width: 100%; min-height: 2.55rem; padding: 0.55rem 0.68rem; border: 1px solid #c9d4dc; border-radius: 8px; background: var(--vp-c-bg); color: var(--vp-c-text-1); font: inherit; }
.control-panel input:focus, .control-panel select:focus { border-color: var(--demo-teal); outline: 3px solid rgba(12, 124, 134, 0.13); }
.wide { grid-column: span 2; }
.actions { display: flex; flex-direction: row; align-items: center; gap: 0.7rem; }
button { min-height: 2.35rem; padding: 0.45rem 0.8rem; border: 1px solid #bccbd4; border-radius: 8px; background: var(--vp-c-bg); color: var(--demo-ink); font-weight: 700; cursor: pointer; }
button:hover:not(:disabled) { border-color: var(--demo-teal); color: var(--demo-teal-dark); }
button:focus-visible { outline: 3px solid rgba(12, 124, 134, 0.22); outline-offset: 2px; }
button:disabled { opacity: 0.5; cursor: wait; }
button.primary { border-color: var(--demo-teal); background: var(--demo-teal); color: white; }
.action-note { min-width: 0; color: var(--demo-muted); font-size: 0.82rem; overflow-wrap: anywhere; }
.error-panel { margin: 1rem 0; padding: 0.9rem 1rem; border-left: 4px solid #c33d3d; border-radius: 6px; background: #fff1f0; color: #732626; }
.error-panel a { margin-left: 0.3rem; font-weight: 700; }
.viewer-panel { margin-top: 1.2rem; }
.viewer-toolbar { display: flex; justify-content: space-between; align-items: center; gap: 1rem; margin-bottom: 0.55rem; }
.viewer-toolbar > div:first-child { display: flex; flex-direction: column; }
.viewer-toolbar span { color: var(--demo-muted); font-size: 0.78rem; }
.view-actions { display: flex; gap: 0.35rem; }
.view-actions button { min-width: 2.35rem; }
.viewer-host { min-height: 520px; }
.semantics-note { margin: 0.65rem 0 0; padding: 0.75rem 0.9rem; border-radius: 9px; background: rgba(117, 98, 168, 0.09); color: var(--demo-muted); font-size: 0.86rem; }
.metrics-grid { display: grid; grid-template-columns: repeat(4, 1fr); gap: 0.85rem; margin-top: 1rem; }
.metrics-grid article { padding: 0.9rem 1rem; border: 1px solid var(--demo-line); border-radius: 12px; background: var(--vp-c-bg); }
.metrics-grid h2 { margin: 0 0 0.55rem; color: var(--demo-teal-dark); font-size: 0.78rem; letter-spacing: 0.08em; text-transform: uppercase; }
dl { margin: 0; }
dl div { display: flex; justify-content: space-between; gap: 1rem; padding: 0.24rem 0; border-bottom: 1px dotted var(--demo-line); }
dt { color: var(--demo-muted); font-size: 0.78rem; }
dd { margin: 0; font: 700 0.78rem/1.4 ui-monospace, monospace; text-align: right; }
.budget-note { margin: 0.55rem 0 0; color: var(--demo-coral); font-size: 0.75rem; }
.trace-panel { margin-top: 1rem; border: 1px solid var(--demo-line); border-radius: 12px; padding: 0.8rem 1rem; }
.trace-panel summary { cursor: pointer; font-weight: 700; }
.hash { overflow-wrap: anywhere; font: 0.76rem/1.5 ui-monospace, monospace; }
.trace-table-wrap { overflow-x: auto; }
table { width: 100%; border-collapse: collapse; font: 0.78rem/1.4 ui-monospace, monospace; }
th, td { padding: 0.45rem; border-bottom: 1px solid var(--demo-line); text-align: left; }
@media (max-width: 1000px) {
  .control-panel { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  .metrics-grid { grid-template-columns: repeat(2, 1fr); }
}
@media (max-width: 640px) {
  .demo-hero, .viewer-toolbar { flex-direction: column; }
  .control-panel, .metrics-grid { grid-template-columns: 1fr; }
  .wide { grid-column: auto; }
  .actions { align-items: stretch; flex-direction: column; }
  .viewer-toolbar { align-items: stretch; }
  .view-actions { flex-wrap: wrap; }
}
:global(.dark) .demo-shell { --demo-ink: #e8eef5; --demo-muted: #aab8c8; --demo-paper: #171b22; --demo-line: #34404b; --demo-teal-dark: #5ecbd2; }
:global(.dark) .error-panel { background: #351f21; color: #ffc8c5; }
</style>
