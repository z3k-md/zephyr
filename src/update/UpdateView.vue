<script setup lang="ts">
  import { computed, onMounted, onUnmounted, ref } from 'vue';
  import { getVersion } from '@tauri-apps/api/app';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { updateInstallNow, updateLater, updateStatus } from '../api';
  import type { UpdateStatus } from '../types';

  // The update card in the screen's corner: the download's progress, then a short countdown
  // to installing, with Install now and Later.

  const status = ref<UpdateStatus | null>(null);
  const now = ref(Date.now());
  /** The version running now, shown beside the one being installed. */
  const running = ref('');
  let unlisten: UnlistenFn | null = null;
  let clock = 0;

  onMounted(async () => {
    document.documentElement.classList.add('update-page');
    void getVersion()
      .then((version) => (running.value = version))
      .catch(() => undefined);
    unlisten = await listen<UpdateStatus>('update-status', (event) => {
      status.value = event.payload;
    });
    status.value = (await updateStatus().catch(() => null)) ?? status.value;
    clock = window.setInterval(() => (now.value = Date.now()), 100);
  });

  onUnmounted(() => {
    unlisten?.();
    window.clearInterval(clock);
  });

  function megabytes(bytes: number) {
    return (bytes / 1024 / 1024).toFixed(1);
  }

  const title = computed(() => {
    const current = status.value;
    if (!current) return 'Zephyr update';
    if (current.phase === 'failed') return 'Update failed';
    return running.value
      ? `Zephyr ${running.value} → ${current.version}`
      : `Zephyr ${current.version}`;
  });

  /** 0 to 1 for the bar; null when the size isn't known yet. */
  const fraction = computed(() => {
    const current = status.value;
    if (!current) return 0;
    if (current.phase === 'downloading') {
      return current.total ? Math.min(1, current.downloaded / current.total) : null;
    }
    if (current.phase === 'ready') {
      // The bar drains as the countdown runs.
      return Math.max(0, Math.min(1, (current.installAt - now.value) / 5000));
    }
    return 1;
  });

  const seconds = computed(() =>
    status.value?.phase === 'ready'
      ? Math.max(0, Math.ceil((status.value.installAt - now.value) / 1000))
      : 0
  );

  const detail = computed(() => {
    const current = status.value;
    if (!current) return '';
    switch (current.phase) {
      case 'downloading':
        return current.total
          ? `Downloading · ${megabytes(current.downloaded)} of ${megabytes(current.total)} MB`
          : `Downloading · ${megabytes(current.downloaded)} MB`;
      case 'ready':
        return `Installing in ${seconds.value}…`;
      case 'installing':
        return 'Installing. Zephyr reopens in a moment.';
      case 'failed':
        return current.message;
    }
    return '';
  });

  const percent = computed(() =>
    status.value?.phase === 'downloading' && fraction.value !== null
      ? `${Math.floor(fraction.value * 100)}%`
      : ''
  );
</script>

<template>
  <div class="update-card" :class="status?.phase">
    <div class="update-head">
      <svg class="update-icon" viewBox="0 0 24 24" aria-hidden="true">
        <path d="M12 4v11M7.5 10.5 12 15l4.5-4.5M5 19.5h14" />
      </svg>
      <div class="update-text">
        <strong>{{ title }}</strong>
        <span>{{ detail }}</span>
      </div>
      <span v-if="percent" class="update-percent">{{ percent }}</span>
    </div>
    <div
      v-if="status?.phase !== 'failed'"
      class="update-bar"
      :class="{ unknown: fraction === null }"
      role="progressbar"
      :aria-valuenow="fraction === null ? undefined : Math.round(fraction * 100)"
      aria-valuemin="0"
      aria-valuemax="100"
    >
      <span :style="{ width: `${(fraction ?? 0.3) * 100}%` }" />
    </div>
    <div class="update-actions">
      <button
        v-if="status?.phase === 'downloading' || status?.phase === 'ready'"
        type="button"
        @click="updateLater()"
      >
        Later
      </button>
      <button v-if="status?.phase === 'failed'" type="button" @click="updateLater()">Close</button>
      <button
        v-if="status?.phase === 'ready'"
        type="button"
        class="primary"
        @click="updateInstallNow()"
      >
        Install now
      </button>
    </div>
  </div>
</template>
