<script setup lang="ts">
// biome-ignore-all lint/correctness/noUnusedVariables: Vue template bindings are consumed outside the script AST.
import type { ArchiveInfo } from "pangenome-range/reader";
import { ref, watch } from "vue";
import type { ArchiveSourceSelection } from "./types";

const props = defineProps<{
  open: boolean;
  presets: readonly ArchiveSourceSelection[];
  activeId: ArchiveSourceSelection["id"];
  activeLabel: string;
  info?: ArchiveInfo;
}>();
const emit = defineEmits<{
  close: [];
  select: [source: ArchiveSourceSelection];
}>();
const customUrl = ref("");
const fileInput = ref<HTMLInputElement>();
const selectedPreset = ref("");

watch(
  () => [props.open, props.activeId, props.presets] as const,
  () => {
    if (!props.open) return;
    selectedPreset.value =
      props.presets.find((source) => source.id === props.activeId)?.id ??
      props.presets[0]?.id ??
      "";
  },
  { immediate: true },
);

function openPreset(): void {
  const preset = props.presets.find(
    (source) => source.id === selectedPreset.value,
  );
  if (preset !== undefined) emit("select", preset);
}

function openCustom(): void {
  const value = customUrl.value.trim();
  if (value.length === 0) return;
  emit("select", {
    id: "custom",
    source: value,
    label: "Custom remote archive",
    key: `url:${value}`,
  });
}

function chooseFile(event: Event): void {
  const file = (event.target as HTMLInputElement).files?.[0];
  if (file === undefined) return;
  emit("select", {
    id: "local",
    source: file,
    label: file.name,
    key: `file:${file.name}:${file.size}`,
  });
}
</script>

<template>
  <div v-if="open" class="archive-menu" role="dialog" aria-label="Archive source">
    <header><div><span>Archive source</span><h2>Static .pngr object</h2></div><button type="button" aria-label="Close archive source" @click="emit('close')">×</button></header>
    <div class="archive-menu__active"><strong>{{ activeLabel }}</strong><span>{{ info ? `${Number(info.archiveBytes / 1024n / 1024n).toLocaleString()} MiB · format v${info.formatVersion}` : 'Opening…' }}</span></div>
    <label><span>Demo archive</span><select v-model="selectedPreset" aria-label="Demo archive"><option v-for="preset in presets" :key="preset.id" :value="preset.id">{{ preset.label }}</option></select></label>
    <p v-if="presets.find((source) => source.id === selectedPreset)?.description" class="archive-menu__note">{{ presets.find((source) => source.id === selectedPreset)?.description }}</p>
    <button type="button" :disabled="selectedPreset.length === 0 || selectedPreset === activeId" @click="openPreset">{{ selectedPreset === activeId ? 'Currently open' : 'Open selected demo' }}</button>
    <label><span>Remote .pngr URL</span><input v-model="customUrl" type="url" placeholder="https://…/archive.pngr" /></label>
    <button type="button" @click="openCustom">Open remote URL</button>
    <button type="button" @click="fileInput?.click()">Open local file</button>
    <input ref="fileInput" class="visually-hidden" type="file" accept=".pngr,application/octet-stream" @change="chooseFile" />
    <p>Files stay in this browser. Remote archives are read with HTTP byte ranges; no query server is used.</p>
  </div>
</template>
