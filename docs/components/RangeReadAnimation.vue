<script setup lang="ts">
// biome-ignore-all lint/correctness/noUnusedVariables: Vue template bindings are consumed outside the script AST.
import { computed, onMounted, onUnmounted, ref, watch } from "vue";

export type RangeReadAnimationApi = {
  readonly duration: number;
  readonly ready: boolean;
  time: number;
  completed: boolean;
  setTime: (ms: number) => void;
  play: () => void;
  pause: () => void;
  playFromStart: () => void;
};

declare global {
  interface Window {
    __pngrRangeReadAnimation?: RangeReadAnimationApi;
  }
}

const props = withDefaults(
  defineProps<{
    capture?: boolean;
    autoplay?: boolean;
  }>(),
  {
    capture: false,
    autoplay: true,
  },
);

const DURATION = 13_000;
const GAP = 4;
const FILE = { x: 28, y: 118, w: 904, h: 46 };
const QUERY_START = 31_353_194;
const BUCKET_SPAN = 16_384 * 32;
const PAGE_INDEX = Math.floor(QUERY_START / BUCKET_SPAN);

const SCENES = [
  {
    id: "layout",
    start: 0,
    end: 1300,
    round: 0,
    roundLabel: "The object",
    caption:
      "One immutable .pngr file: 64-byte header, root, optional extensions, fixed 4 KiB directory pages, independently compressed tiles.",
  },
  {
    id: "query",
    start: 1300,
    end: 2500,
    round: 0,
    roundLabel: "The query",
    caption:
      "The browser wants GRCh38 chr6:31,353,194–31,367,067. It will not download the 8.8 GB object.",
  },
  {
    id: "bootstrap",
    start: 2500,
    end: 4500,
    round: 1,
    roundLabel: "Round 1 · bootstrap",
    caption:
      "Range bytes=0–16383. Decode PNGRNG01, then the root. The chr6 manifest names the directory span.",
  },
  {
    id: "arithmetic",
    start: 4500,
    end: 6400,
    round: 1,
    roundLabel: "Arithmetic lookup",
    caption:
      "page = first + floor((start − grid) / bucket_span) × 4096. Coordinates become a byte offset. No tree walk.",
  },
  {
    id: "directory",
    start: 6400,
    end: 8300,
    round: 2,
    roundLabel: "Round 2 · directory",
    caption:
      "Read one 4 KiB page. Each 56-byte entry names a payload offset, length, and BLAKE3-128.",
  },
  {
    id: "payloads",
    start: 8300,
    end: 10_500,
    round: 3,
    roundLabel: "Round 3 · payloads",
    caption:
      "Fetch only the overlapping tiles, in parallel. Each is an independent zstd frame.",
  },
  {
    id: "result",
    start: 10_500,
    end: 13_000,
    round: 3,
    roundLabel: "Done",
    caption:
      "5 range reads · 74 KB of 8.8 GB. No query server. The unread remainder stays on the origin.",
  },
] as const;

const SECTION_DEFS = [
  { id: "header", label: "header", sub: "64 B", w: 78 },
  { id: "root", label: "root", sub: "manifests", w: 96 },
  { id: "ext", label: "ext", sub: "optional", w: 72 },
  { id: "dir", label: "directory pages", sub: "4 KiB each", w: 312 },
  { id: "pay", label: "regional payloads", sub: "independent zstd", w: 330 },
] as const;

const REQUESTS = [
  {
    id: "boot",
    from: 2500,
    layer: "bootstrap",
    range: "bytes=0-16383",
    bytes: "16,384 B",
    parallel: false,
  },
  {
    id: "dir",
    from: 6400,
    layer: "directory",
    range: "bytes=290816-294911",
    bytes: "4,096 B",
    parallel: false,
  },
  {
    id: "p0",
    from: 8500,
    layer: "payload",
    range: "bytes=50331648-50353747",
    bytes: "22,100 B",
    parallel: true,
  },
  {
    id: "p1",
    from: 8500,
    layer: "payload",
    range: "bytes=50353748-50372147",
    bytes: "18,400 B",
    parallel: true,
  },
  {
    id: "p2",
    from: 8500,
    layer: "payload",
    range: "bytes=50372148-50385139",
    bytes: "12,992 B",
    parallel: true,
  },
] as const;

const ENTRIES = [
  {
    core: "31,342,592–31,358,976",
    hit: true,
    offset: "50,331,648",
    length: "22,100",
  },
  {
    core: "31,358,976–31,367,168",
    hit: true,
    offset: "50,353,748",
    length: "18,400",
  },
  {
    core: "31,367,168–31,375,360",
    hit: true,
    offset: "50,372,148",
    length: "12,992",
  },
  {
    core: "31,375,360–31,391,744",
    hit: false,
    offset: "50,385,140",
    length: "9,410",
  },
] as const;

const time = ref(0);
const playing = ref(false);
const completed = ref(false);
const captureQuery = ref(false);
const reducedMotion = ref(false);
const rootEl = ref<HTMLElement | null>(null);
const captureRoot = ref<HTMLElement | null>(null);

let raf = 0;
let lastTs = 0;
let observer: IntersectionObserver | null = null;

const captureMode = computed(() => props.capture || captureQuery.value);

const scene = computed(() => {
  const t = time.value;
  return SCENES.find((item) => t < item.end) ?? SCENES[SCENES.length - 1];
});

const sections = computed(() => {
  let x = FILE.x;
  return SECTION_DEFS.map((def) => {
    const next = { ...def, x, y: FILE.y, h: FILE.h };
    x += def.w + GAP;
    return next;
  });
});

const dirSection = computed(
  () => sections.value.find((item) => item.id === "dir") ?? sections.value[3],
);
const paySection = computed(
  () => sections.value.find((item) => item.id === "pay") ?? sections.value[4],
);

const bootstrapWidth = computed(() => {
  const ext = sections.value[2];
  return ext.x + ext.w - FILE.x;
});

const dirTicks = computed(() => {
  const dir = dirSection.value;
  const count = 16;
  const inner = dir.w - 10;
  const w = inner / count;
  return Array.from({ length: count }, (_, index) => ({
    index,
    x: dir.x + 5 + index * w,
    w: w - 1.6,
    selected: index === 9,
  }));
});

const payloadTiles = computed(() => {
  const pay = paySection.value;
  const count = 18;
  const inner = pay.w - 10;
  const w = inner / count;
  const selected = new Set([5, 6, 7]);
  return Array.from({ length: count }, (_, index) => ({
    index,
    x: pay.x + 5 + index * w,
    w: w - 1.6,
    selected: selected.has(index),
  }));
});

const selectedDirTick = computed(
  () => dirTicks.value.find((tick) => tick.selected) ?? dirTicks.value[9],
);

const visibleRequests = computed(() =>
  REQUESTS.filter((request) => time.value >= request.from),
);

const bytesRead = computed(() => {
  let total = 0;
  if (time.value >= 2500) total += 16_384;
  if (time.value >= 6400) total += 4_096;
  if (time.value >= 8500) total += 22_100 + 18_400 + 12_992;
  return total;
});

const bytesLabel = computed(() => {
  if (bytesRead.value >= 70_000) return "73,972 B of 8.8 GB";
  if (bytesRead.value === 0) return "0 B of 8.8 GB";
  return `${bytesRead.value.toLocaleString("en-US")} B of 8.8 GB`;
});

const meterRatio = computed(() => {
  const max = 73_972;
  return Math.min(1, bytesRead.value / max);
});

const sceneProgress = computed(() => {
  const current = scene.value;
  const span = current.end - current.start;
  if (span <= 0) return 1;
  return clamp((time.value - current.start) / span, 0, 1);
});

const showQuery = computed(() => time.value >= 1300);
const showBootstrap = computed(() => time.value >= 2500);
const showArithmetic = computed(() => time.value >= 4500);
const showDirectory = computed(() => time.value >= 6400);
const showPayloads = computed(() => time.value >= 8300);

const packetX = computed(() => {
  const local = sceneProgress.value;
  if (scene.value.id === "bootstrap") {
    return FILE.x + 8 + local * (bootstrapWidth.value - 36);
  }
  if (scene.value.id === "directory") {
    const tick = selectedDirTick.value;
    return tick.x + tick.w / 2;
  }
  return FILE.x;
});

const caption = computed(() => scene.value.caption);
const roundLabel = computed(() => scene.value.roundLabel);
const sceneId = computed(() => scene.value.id);

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function sceneOpacity(id: (typeof SCENES)[number]["id"], hold = false): number {
  const current = scene.value;
  if (hold) {
    const starts: Record<string, number> = {
      layout: 0,
      query: 1300,
      bootstrap: 2500,
      arithmetic: 4500,
      directory: 6400,
      payloads: 8300,
      result: 10_500,
    };
    const start = starts[id] ?? 0;
    if (time.value < start) return 0;
    return clamp((time.value - start) / 180, 0, 1);
  }
  if (current.id !== id) return 0;
  if (sceneProgress.value <= 0.92) return 1;
  return clamp((1 - sceneProgress.value) / 0.08, 0, 1);
}

function toggle(): void {
  if (playing.value) pause();
  else play();
}

function play(): void {
  if (captureMode.value && !playing.value && time.value >= DURATION) {
    time.value = 0;
    completed.value = false;
  }
  if (time.value >= DURATION) {
    time.value = 0;
    completed.value = false;
  }
  if (playing.value) return;
  playing.value = true;
  lastTs = 0;
  raf = requestAnimationFrame(tick);
}

function pause(): void {
  playing.value = false;
  if (raf !== 0) {
    cancelAnimationFrame(raf);
    raf = 0;
  }
}

function restart(): void {
  time.value = 0;
  completed.value = false;
  play();
}

function seekScene(index: number): void {
  const next = SCENES[index];
  if (next === undefined) return;
  pause();
  time.value = next.start + 80;
}

function setTime(ms: number): void {
  pause();
  time.value = clamp(ms, 0, DURATION);
  completed.value = time.value >= DURATION;
}

function playFromStart(): void {
  time.value = 0;
  completed.value = false;
  play();
}

function tick(ts: number): void {
  if (!playing.value) {
    raf = 0;
    return;
  }
  if (lastTs === 0) lastTs = ts;
  const dt = Math.min(48, ts - lastTs);
  lastTs = ts;
  time.value += dt;
  if (time.value >= DURATION) {
    if (captureMode.value) {
      time.value = DURATION;
      completed.value = true;
      pause();
      return;
    }
    time.value = 0;
  }
  raf = requestAnimationFrame(tick);
}

function bindWindowApi(): void {
  window.__pngrRangeReadAnimation = {
    duration: DURATION,
    ready: true,
    get time() {
      return time.value;
    },
    set time(ms: number) {
      setTime(ms);
    },
    get completed() {
      return completed.value;
    },
    set completed(value: boolean) {
      completed.value = value;
    },
    setTime,
    play,
    pause,
    playFromStart,
  };
}

onMounted(() => {
  captureQuery.value = new URLSearchParams(window.location.search).has(
    "capture",
  );
  reducedMotion.value = window.matchMedia(
    "(prefers-reduced-motion: reduce)",
  ).matches;
  bindWindowApi();
  if (reducedMotion.value) {
    time.value = DURATION;
    return;
  }
  if (captureMode.value) {
    time.value = 0;
    return;
  }
  observer = new IntersectionObserver(
    (entries) => {
      const visible = entries.some((entry) => entry.isIntersecting);
      if (visible && props.autoplay && !playing.value && !completed.value) {
        play();
      } else if (!visible && playing.value) {
        pause();
      }
    },
    { threshold: 0.35 },
  );
  if (rootEl.value) observer.observe(rootEl.value);
});

onUnmounted(() => {
  pause();
  observer?.disconnect();
  if (window.__pngrRangeReadAnimation !== undefined) {
    delete window.__pngrRangeReadAnimation;
  }
});

watch(captureMode, (value) => {
  if (value) pause();
});
</script>

<template>
  <figure
    ref="rootEl"
    class="range-read-animation"
    :class="{
      'is-capture': captureMode,
      'is-playing': playing,
    }"
    :data-scene="sceneId"
    :data-time="Math.round(time)"
    tabindex="0"
    @keydown.space.prevent="toggle"
  >
    <div ref="captureRoot" class="range-read-animation__capture">
      <svg
        class="range-read-animation__svg"
        viewBox="0 0 960 430"
        role="img"
        :aria-label="caption"
      >
        <rect class="rr-bg" x="0" y="0" width="960" height="430" rx="0" />

        <text class="rr-kicker" x="28" y="28">.pngr range query</text>
        <text class="rr-title" x="28" y="52">
          How the file is laid out so HTTP Range works
        </text>
        <rect
          class="rr-badge"
          x="738"
          y="18"
          width="194"
          height="28"
          rx="14"
        />
        <text class="rr-badge-text" x="835" y="37" text-anchor="middle">
          {{ roundLabel }}
        </text>

        <g :opacity="showQuery ? 1 : 0.35">
          <rect
            class="rr-query"
            x="28"
            y="66"
            width="904"
            height="28"
            rx="8"
          />
          <text class="rr-query-text" x="42" y="85">
            query  GRCh38  ·  chr6  ·  31,353,194–31,367,067
          </text>
          <text class="rr-query-note" x="916" y="85" text-anchor="end">
            HLA-B
          </text>
        </g>

        <text class="rr-file-label" x="28" y="110">static object · not to scale</text>
        <text class="rr-file-name" x="932" y="110" text-anchor="end">
          archive.pngr
        </text>

        <g
          v-for="section in sections"
          :key="section.id"
          :class="`rr-sec rr-sec--${section.id}`"
        >
          <rect
            :x="section.x"
            :y="FILE.y"
            :width="section.w"
            :height="FILE.h"
            rx="8"
          />
          <text
            class="rr-sec-label"
            :x="section.x + section.w / 2"
            :y="FILE.y + 20"
            text-anchor="middle"
          >
            {{ section.label }}
          </text>
          <text
            class="rr-sec-sub"
            :x="section.x + section.w / 2"
            :y="FILE.y + 34"
            text-anchor="middle"
          >
            {{ section.sub }}
          </text>
        </g>

        <g class="rr-dir-ticks" :opacity="0.9">
          <rect
            v-for="tick in dirTicks"
            :key="`d-${tick.index}`"
            :class="{ 'is-selected': tick.selected && showArithmetic }"
            :x="tick.x"
            :y="FILE.y + 36"
            :width="tick.w"
            height="8"
            rx="1"
          />
        </g>
        <g class="rr-pay-ticks">
          <rect
            v-for="tile in payloadTiles"
            :key="`p-${tile.index}`"
            :class="{ 'is-selected': tile.selected && showPayloads }"
            :x="tile.x"
            :y="FILE.y + 36"
            :width="tile.w"
            height="8"
            rx="1"
          />
        </g>

        <rect
          class="rr-bootstrap"
          :x="FILE.x - 3"
          :y="FILE.y - 3"
          :width="bootstrapWidth + 6"
          :height="FILE.h + 6"
          rx="10"
          :opacity="showBootstrap && !showDirectory ? 1 : showBootstrap ? 0.35 : 0"
        />

        <rect
          v-if="showDirectory"
          class="rr-page-hit"
          :x="selectedDirTick.x - 2"
          :y="FILE.y - 4"
          :width="selectedDirTick.w + 4"
          :height="FILE.h + 8"
          rx="4"
        />

        <g v-if="sceneId === 'bootstrap'">
          <circle class="rr-packet" :cx="packetX" :cy="FILE.y - 10" r="5" />
        </g>

        <g :opacity="sceneOpacity('layout')">
          <text class="rr-panel-title" x="28" y="196">
            Layout is the query plan
          </text>
          <text class="rr-panel-body" x="28" y="220">
            Header is always 64 bytes at offset 0. Root starts at 64 and names
            each real reference.
          </text>
          <text class="rr-panel-body" x="28" y="242">
            Directory pages are contiguous 4 KiB buckets. Payloads are
            addressable byte ranges after data_offset.
          </text>
          <text class="rr-panel-body" x="28" y="264">
            A CDN or object store can serve this with ordinary HTTP Range.
            No GraphQL, no query daemon.
          </text>
        </g>

        <g :opacity="sceneOpacity('query', true) * (sceneId === 'query' ? 1 : 0)">
          <text class="rr-panel-title" x="28" y="196">
            Interval in, byte ranges out
          </text>
          <text class="rr-panel-body" x="28" y="220">
            The reader looks up sample + contig in the root, intersects the
            requested interval, and computes bucket indexes.
          </text>
          <text class="rr-panel-body" x="28" y="242">
            first = floor((q_start − grid_start) / bucket_span)
          </text>
          <text class="rr-panel-body" x="28" y="264">
            last = floor((q_end − 1 − grid_start) / bucket_span)
          </text>
        </g>

        <g :opacity="sceneOpacity('bootstrap')">
          <text class="rr-panel-title" x="28" y="196">
            16 KiB bootstrap already contains the map
          </text>
          <g class="rr-fields">
            <rect x="28" y="208" width="210" height="54" rx="8" />
            <text x="40" y="228">magic</text>
            <text class="rr-mono" x="40" y="248">PNGRNG01</text>
            <rect x="246" y="208" width="140" height="54" rx="8" />
            <text x="258" y="228">version</text>
            <text class="rr-mono" x="258" y="248">1</text>
            <rect x="394" y="208" width="210" height="54" rx="8" />
            <text x="406" y="228">root offset</text>
            <text class="rr-mono" x="406" y="248">64</text>
            <rect x="612" y="208" width="320" height="54" rx="8" />
            <text x="624" y="228">data_offset</text>
            <text class="rr-mono" x="624" y="248">after every 4 KiB page</text>
          </g>
        </g>

        <g :opacity="sceneOpacity('arithmetic')">
          <text class="rr-panel-title" x="28" y="196">
            Coordinates hash to a page
          </text>
          <text class="rr-formula" x="28" y="228">
            page_offset = first_page + floor((31,353,194 - 0) / 524,288) * 4,096
          </text>
          <text class="rr-formula-result" x="28" y="258">
            floor({{ QUERY_START.toLocaleString("en-US") }} / {{ BUCKET_SPAN.toLocaleString("en-US") }})
            = page {{ PAGE_INDEX }}
          </text>
          <text class="rr-panel-body" x="28" y="286">
            window_size 16,384 · bucket_span = window × 32. Adaptive children
            stay inside that bucket, so one page is enough.
          </text>
        </g>

        <g :opacity="sceneOpacity('directory')">
          <text class="rr-panel-title" x="28" y="196">
            One 4 KiB page · ≤72 entries of 56 bytes
          </text>
          <g
            v-for="(entry, index) in ENTRIES"
            :key="entry.core"
            :class="entry.hit ? 'rr-entry-hit' : 'rr-entry-miss'"
          >
            <rect
              class="rr-card"
              :x="28 + index * 230"
              y="208"
              width="220"
              height="86"
              rx="8"
            />
            <text :x="42 + index * 230" y="230">
              {{ entry.hit ? "overlap" : "outside" }}
            </text>
            <text class="rr-mono" :x="42 + index * 230" y="250">
              {{ entry.core }}
            </text>
            <text class="rr-mono" :x="42 + index * 230" y="270">
              @ {{ entry.offset }}
            </text>
            <text class="rr-mono" :x="42 + index * 230" y="286">
              {{ entry.length }} B · BLAKE3-128
            </text>
          </g>
        </g>

        <g :opacity="sceneOpacity('payloads')">
          <text class="rr-panel-title" x="28" y="196">
            Last round is parallel
          </text>
          <text class="rr-panel-body" x="28" y="220">
            Three independent Range GETs. Verify BLAKE3-128, decompress one
            zstd frame, decode PNGRGN01.
          </text>
          <g v-for="(tile, index) in payloadTiles.filter((item) => item.selected)" :key="`hit-${tile.index}`">
            <path
              class="rr-pay-arrow"
              :d="`M ${tile.x + tile.w / 2} ${FILE.y + FILE.h + 6} L ${140 + index * 260} 250`"
            />
            <rect class="rr-card" :x="28 + index * 260" y="250" width="240" height="52" rx="8" />
            <text class="rr-mono" :x="42 + index * 260" y="272">
              tile {{ PAGE_INDEX }}{{ ["a", "b", "c"][index] }}
            </text>
            <text class="rr-panel-body" :x="42 + index * 260" y="290">
              zstd · PNGRGN01 · local GBWT
            </text>
          </g>
        </g>

        <g :opacity="sceneOpacity('result')">
          <text class="rr-panel-title" x="28" y="196">
            The unread 8.8 GB is never transferred
          </text>
          <g class="rr-stats">
            <rect x="28" y="210" width="220" height="72" rx="8" />
            <text x="44" y="234">dependency rounds</text>
            <text class="rr-stat" x="44" y="266">3</text>
            <rect x="256" y="210" width="220" height="72" rx="8" />
            <text x="272" y="234">HTTP Range reads</text>
            <text class="rr-stat" x="272" y="266">5</text>
            <rect x="484" y="210" width="220" height="72" rx="8" />
            <text x="500" y="234">bytes read</text>
            <text class="rr-stat" x="500" y="266">74 KB</text>
            <rect x="712" y="210" width="220" height="72" rx="8" />
            <text x="728" y="234">archive</text>
            <text class="rr-stat" x="728" y="266">8.8 GB</text>
          </g>
        </g>

        <g class="rr-log">
          <text class="rr-log-title" x="28" y="330">HTTP Range</text>
          <g v-for="(request, index) in visibleRequests" :key="request.id">
            <rect
              :x="28 + (index % 5) * 186"
              y="340"
              width="178"
              height="40"
              rx="6"
            />
            <text class="rr-log-layer" :x="40 + (index % 5) * 186" y="356">
              {{ request.layer }}{{ request.parallel ? " · parallel" : "" }}
            </text>
            <text class="rr-mono rr-log-bytes" :x="40 + (index % 5) * 186" y="372">
              {{ request.bytes }}
            </text>
          </g>
        </g>

        <rect class="rr-meter-track" x="28" y="392" width="760" height="10" rx="5" />
        <rect
          class="rr-meter-fill"
          x="28"
          y="392"
          :width="Math.max(meterRatio * 760, bytesRead > 0 ? 8 : 0)"
          height="10"
          rx="5"
        />
        <text class="rr-meter-label" x="932" y="402" text-anchor="end">
          {{ bytesLabel }}
        </text>
        <text class="rr-caption" x="28" y="422">{{ caption }}</text>
      </svg>
    </div>

    <figcaption class="range-read-animation__live" aria-live="polite">
      {{ roundLabel }} — {{ caption }}
    </figcaption>

    <div v-if="!captureMode" class="range-read-animation__controls">
      <button type="button" @click="toggle">
        {{ playing ? "Pause" : "Play" }}
      </button>
      <button type="button" @click="restart">Restart</button>
      <ol>
        <li v-for="(item, index) in SCENES" :key="item.id">
          <button
            type="button"
            :class="{ 'is-active': sceneId === item.id }"
            :aria-current="sceneId === item.id ? 'step' : undefined"
            @click="seekScene(index)"
          >
            {{ item.roundLabel }}
          </button>
        </li>
      </ol>
    </div>
  </figure>
</template>

<style scoped>
.range-read-animation {
  --rr-bg: #f8fafc;
  --rr-ink: #0f172a;
  --rr-muted: #475569;
  --rr-faint: #94a3b8;
  --rr-line: #cbd5e1;
  --rr-card: #ffffff;
  --rr-header: #0369a1;
  --rr-root: #4338ca;
  --rr-ext: #6d28d9;
  --rr-dir: #047857;
  --rr-pay: #b45309;
  --rr-query: #9d174d;
  --rr-hit: #f59e0b;
  --rr-packet: #0ea5e9;
  --rr-badge: #0f172a;
  --rr-badge-ink: #f8fafc;
  --rr-meter: #0f766e;
  margin: 1.25rem 0 1.75rem;
  padding: 0;
  border: 1px solid var(--vp-c-divider);
  border-radius: 14px;
  background: var(--rr-bg);
  overflow: hidden;
  outline: none;
}

.dark .range-read-animation,
.range-read-animation.is-capture {
  --rr-bg: #0b1220;
  --rr-ink: #e2e8f0;
  --rr-muted: #94a3b8;
  --rr-faint: #64748b;
  --rr-line: #1e293b;
  --rr-card: #121a2b;
  --rr-header: #38bdf8;
  --rr-root: #818cf8;
  --rr-ext: #c084fc;
  --rr-dir: #34d399;
  --rr-pay: #fbbf24;
  --rr-query: #f472b6;
  --rr-hit: #fbbf24;
  --rr-packet: #38bdf8;
  --rr-badge: #38bdf8;
  --rr-badge-ink: #0b1220;
  --rr-meter: #2dd4bf;
}

.range-read-animation.is-capture {
  position: fixed;
  top: 0;
  left: 0;
  z-index: 50;
  border: 0;
  border-radius: 0;
  margin: 0;
  width: 960px;
  background: #0b1220;
}

.range-read-animation.is-capture .range-read-animation__capture {
  width: 960px;
  background: #0b1220;
}

.range-read-animation__capture {
  background: var(--rr-bg);
}

.range-read-animation__svg {
  display: block;
  width: 100%;
  height: auto;
  font-family: var(--vp-font-family-base), ui-sans-serif, system-ui, sans-serif;
}

.rr-bg {
  fill: var(--rr-bg);
}

.rr-kicker {
  fill: var(--rr-muted);
  font-size: 12px;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}

.rr-title {
  fill: var(--rr-ink);
  font-size: 20px;
  font-weight: 650;
}

.rr-badge {
  fill: var(--rr-badge);
}

.rr-badge-text {
  fill: var(--rr-badge-ink);
  font-size: 12px;
  font-weight: 650;
}

.rr-query {
  fill: var(--rr-card);
  stroke: var(--rr-query);
  stroke-width: 1.25;
}

.rr-query-text,
.rr-query-note {
  fill: var(--rr-ink);
  font-size: 13px;
}

.rr-query-note {
  fill: var(--rr-query);
  font-weight: 650;
}

.rr-file-label,
.rr-file-name {
  fill: var(--rr-faint);
  font-size: 11px;
}

.rr-sec rect {
  stroke-width: 1;
}

.rr-sec--header rect {
  fill: color-mix(in srgb, var(--rr-header) 18%, var(--rr-card));
  stroke: var(--rr-header);
}

.rr-sec--root rect {
  fill: color-mix(in srgb, var(--rr-root) 18%, var(--rr-card));
  stroke: var(--rr-root);
}

.rr-sec--ext rect {
  fill: color-mix(in srgb, var(--rr-ext) 16%, var(--rr-card));
  stroke: var(--rr-ext);
}

.rr-sec--dir rect {
  fill: color-mix(in srgb, var(--rr-dir) 16%, var(--rr-card));
  stroke: var(--rr-dir);
}

.rr-sec--pay rect {
  fill: color-mix(in srgb, var(--rr-pay) 16%, var(--rr-card));
  stroke: var(--rr-pay);
}

.rr-sec-label {
  fill: var(--rr-ink);
  font-size: 13px;
  font-weight: 650;
}

.rr-sec-sub {
  fill: var(--rr-muted);
  font-size: 11px;
}

.rr-dir-ticks rect,
.rr-pay-ticks rect {
  fill: color-mix(in srgb, var(--rr-ink) 18%, transparent);
}

.rr-dir-ticks rect.is-selected {
  fill: var(--rr-dir);
}

.rr-pay-ticks rect.is-selected {
  fill: var(--rr-pay);
}

.rr-bootstrap {
  fill: none;
  stroke: var(--rr-header);
  stroke-width: 2;
  stroke-dasharray: 5 3;
}

.rr-page-hit {
  fill: none;
  stroke: var(--rr-dir);
  stroke-width: 2.25;
}

.rr-packet {
  fill: var(--rr-packet);
}

.rr-panel-title {
  fill: var(--rr-ink);
  font-size: 16px;
  font-weight: 650;
}

.rr-panel-body {
  fill: var(--rr-muted);
  font-size: 13px;
}

.rr-card {
  fill: var(--rr-card);
  stroke: var(--rr-line);
}

.rr-entry-miss {
  opacity: 0.45;
}

.rr-fields rect,
.rr-stats rect,
.rr-log rect,
.rr-entry-hit rect {
  fill: var(--rr-card);
  stroke: var(--rr-line);
}

.rr-fields text:not(.rr-mono) {
  fill: var(--rr-muted);
  font-size: 11px;
}

.rr-mono {
  fill: var(--rr-ink);
  font-size: 13px;
  font-family: var(--vp-font-family-mono), ui-monospace, monospace;
}

.rr-formula,
.rr-formula-result {
  fill: var(--rr-ink);
  font-size: 15px;
  font-family: var(--vp-font-family-mono), ui-monospace, monospace;
}

.rr-formula-result {
  fill: var(--rr-dir);
  font-weight: 650;
}

.rr-entry-hit rect {
  stroke: var(--rr-dir);
}

g:not(.rr-entry-hit) .rr-mono {
  fill: var(--rr-ink);
}

.rr-pay-arrow {
  fill: none;
  stroke: var(--rr-pay);
  stroke-width: 1.5;
}

.rr-stats text:not(.rr-stat) {
  fill: var(--rr-muted);
  font-size: 12px;
}

.rr-stat {
  fill: var(--rr-ink);
  font-size: 26px;
  font-weight: 700;
}

.rr-log-title {
  fill: var(--rr-faint);
  font-size: 11px;
  letter-spacing: 0.06em;
  text-transform: uppercase;
}

.rr-log-layer {
  fill: var(--rr-muted);
  font-size: 11px;
}

.rr-log-bytes {
  font-size: 12px;
}

.rr-meter-track {
  fill: var(--rr-line);
}

.rr-meter-fill {
  fill: var(--rr-meter);
}

.rr-meter-label,
.rr-caption {
  fill: var(--rr-ink);
  font-size: 12px;
}

.rr-caption {
  fill: var(--rr-muted);
}

.range-read-animation__live {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  clip: rect(0 0 0 0);
}

.range-read-animation__controls {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem 0.75rem;
  align-items: center;
  padding: 0.75rem 1rem 1rem;
  border-top: 1px solid var(--vp-c-divider);
  background: var(--vp-c-bg);
}

.range-read-animation__controls button {
  appearance: none;
  border: 1px solid var(--vp-c-divider);
  background: var(--vp-c-bg-soft);
  color: var(--vp-c-text-1);
  border-radius: 999px;
  padding: 0.3rem 0.8rem;
  font: inherit;
  font-size: 0.85rem;
  cursor: pointer;
}

.range-read-animation__controls button.is-active {
  background: var(--vp-c-brand-1);
  border-color: transparent;
  color: var(--vp-c-bg);
}

.range-read-animation__controls ol {
  display: flex;
  flex-wrap: wrap;
  gap: 0.4rem;
  list-style: none;
  margin: 0;
  padding: 0;
}

@media (max-width: 720px) {
  .rr-title {
    font-size: 16px;
  }
}
</style>
