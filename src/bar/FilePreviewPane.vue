<script setup lang="ts">
  import { ref, watch } from 'vue';
  import { errorMessage, filePreview } from '../api';
  import type { FilePreview } from '../types';
  import RowIcon from './RowIcon.vue';

  // The preview beside the results (Shift+Space on a file): a picture, the first lines of a
  // text file, or the file's size, date and folder.
  const props = defineProps<{ path: string; name: string; icon?: string }>();

  const preview = ref<FilePreview | null>(null);
  const error = ref<string | null>(null);
  let generation = 0;

  watch(
    () => props.path,
    async (path) => {
      const current = ++generation;
      preview.value = null;
      error.value = null;
      try {
        const loaded = await filePreview(path);
        if (current === generation) preview.value = loaded;
      } catch (err) {
        if (current === generation) error.value = errorMessage(err);
      }
    },
    { immediate: true }
  );

  function size(bytes: number): string {
    if (bytes < 1024) return `${bytes} bytes`;
    const units = ['KB', 'MB', 'GB', 'TB'];
    let value = bytes / 1024;
    let unit = 0;
    while (value >= 1024 && unit < units.length - 1) {
      value /= 1024;
      unit += 1;
    }
    return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${units[unit]}`;
  }

  function date(seconds: number): string {
    if (!seconds) return '';
    return new Date(seconds * 1000).toLocaleString(undefined, {
      dateStyle: 'medium',
      timeStyle: 'short',
    });
  }

  function folderOf(path: string): string {
    const cut = Math.max(path.lastIndexOf('\\'), path.lastIndexOf('/'));
    return cut > 0 ? path.slice(0, cut) : path;
  }
</script>

<template>
  <aside class="file-preview" aria-label="Preview">
    <header class="file-preview-head">
      <strong>{{ name }}</strong>
      <span v-if="preview && !(preview.kind === 'info' && preview.folder)" class="hint-text">
        {{ size(preview.size) }}
      </span>
    </header>
    <div class="file-preview-body">
      <p v-if="error" class="hint-text">{{ error }}</p>
      <img v-else-if="preview?.kind === 'image'" :src="preview.url" alt="" />
      <pre
        v-else-if="preview?.kind === 'text'">{{ preview.text }}<template v-if="preview.truncated">
…</template></pre>
      <div v-else-if="preview?.kind === 'info'" class="file-preview-info">
        <RowIcon :icon="icon" :label="name" :size="20" />
        <dl>
          <dt>Kind</dt>
          <dd>{{ preview.folder ? 'Folder' : 'File' }}</dd>
          <template v-if="!preview.folder">
            <dt>Size</dt>
            <dd>{{ size(preview.size) }}</dd>
          </template>
          <dt>Modified</dt>
          <dd>{{ date(preview.modified) }}</dd>
          <dt>Where</dt>
          <dd class="file-preview-path">{{ folderOf(path) }}</dd>
        </dl>
      </div>
    </div>
    <footer v-if="preview && preview.kind !== 'info'" class="file-preview-foot hint-text">
      Modified {{ date(preview.modified) }}
    </footer>
  </aside>
</template>
