<script setup lang="ts">
// biome-ignore-all lint/correctness/noUnusedVariables: Vue template bindings are consumed outside the script AST.
import { nextTick, ref, watch } from "vue";

const props = defineProps<{
  open: boolean;
  url: string;
}>();
const emit = defineEmits<{
  close: [];
}>();
const linkInput = ref<HTMLInputElement>();
const copied = ref(false);
const copyFailed = ref(false);

watch(
  () => props.open,
  async (open) => {
    if (!open) return;
    copied.value = false;
    copyFailed.value = false;
    await nextTick();
    linkInput.value?.focus();
    linkInput.value?.select();
  },
);

async function copyLink(): Promise<void> {
  let succeeded = false;
  try {
    await Promise.race([
      navigator.clipboard.writeText(props.url),
      new Promise<never>((_, reject) =>
        setTimeout(() => reject(new Error("Clipboard write timed out")), 800),
      ),
    ]);
    succeeded = true;
  } catch {
    linkInput.value?.focus();
    linkInput.value?.select();
    succeeded = document.execCommand("copy");
  }
  copied.value = succeeded;
  copyFailed.value = !succeeded;
}
</script>

<template>
  <Teleport to="body">
    <div v-if="open" class="share-dialog-backdrop" @click.self="emit('close')" @keydown.esc.stop="emit('close')">
      <section class="share-dialog" role="dialog" aria-modal="true" aria-labelledby="share-dialog-title">
        <header>
          <div><span>SHARE THIS VIEW</span><h2 id="share-dialog-title">Copy exact region link</h2></div>
          <button type="button" aria-label="Close share dialog" @click="emit('close')">×</button>
        </header>
        <p>The link preserves the current reference and genomic interval.</p>
        <div class="share-dialog__link">
          <input ref="linkInput" :value="url" readonly aria-label="Shareable region link" @focus="($event.target as HTMLInputElement).select()" />
          <button type="button" @click="copyLink">{{ copied ? 'Copied' : 'Copy link' }}</button>
        </div>
        <p class="share-dialog__copy-status" aria-live="polite">{{ copied ? 'Link copied to the clipboard.' : copyFailed ? 'Clipboard access is blocked. The link is selected; press Command/Ctrl+C.' : '' }}</p>
      </section>
    </div>
  </Teleport>
</template>
