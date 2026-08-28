<script setup lang="ts">
// biome-ignore-all lint/correctness/noUnusedVariables: Vue template bindings are consumed outside the script AST.
import type { ArchiveInfo } from "pangenome-range/reader";
import { ref } from "vue";
import type { ArchiveSourceSelection } from "./types";

const props = defineProps<{
  open: boolean;
  configuredUrl: string;
  configuredLabel: string;
  activeLabel: string;
  info?: ArchiveInfo;
}>();
const emit = defineEmits<{
  close: [];
  select: [source: ArchiveSourceSelection];
}>();
const customUrl = ref("");
const fileInput = ref<HTMLInputElement>();

function openConfigured(): void {
  if (props.configuredUrl.length === 0) return;
  emit("select", {
    source: props.configuredUrl,
    label: props.configuredLabel,
    key: `url:${props.configuredUrl}`,
  });
}

function openCustom(): void {
  const value = customUrl.value.trim();
  if (value.length === 0) return;
  emit("select", {
    source: value,
    label: "Custom remote archive",
    key: `url:${value}`,
  });
}

function chooseFile(event: Event): void {
  const file = (event.target as HTMLInputElement).files?.[0];
  if (file === undefined) return;
  emit("select", {
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
    <button type="button" :disabled="configuredUrl.length === 0" @click="openConfigured">Open configured archive</button>
    <label><span>Remote .pngr URL</span><input v-model="customUrl" type="url" placeholder="https://…/archive.pngr" /></label>
    <button type="button" @click="openCustom">Open remote URL</button>
    <button type="button" @click="fileInput?.click()">Open local file</button>
    <input ref="fileInput" class="visually-hidden" type="file" accept=".pngr,application/octet-stream" @change="chooseFile" />
    <p>Files stay in this browser. Remote archives are read with HTTP byte ranges; no query server is used.</p>
  </div>
</template>
