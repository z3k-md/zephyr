<script setup lang="ts">
  import './osd.css';
  import { computed, onMounted, onUnmounted, ref } from 'vue';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import RowIcon from '../bar/RowIcon.vue';
  import SpeakerIcon from '../bar/SpeakerIcon.vue';
  import { GLYPHS } from '../bar/glyphs';
  import { osdOpenSound, osdWheel } from '../api';
  import type { VolumeShown } from '../types';

  // The volume overlay, at the screen's corner while the bar is closed: the keys' target and
  // its level, over the apps Alt+volume steps through. Rust shows and hides the window; the
  // wheel over it moves the level, and a click opens the Sound view.

  /** How many apps the strip shows, the target always among them. */
  const STRIP = 8;

  const shown = ref<VolumeShown | null>(null);
  /** Bumped on every press, so the fade-in replays. */
  const pulse = ref(0);
  let unlisten: UnlistenFn | null = null;

  onMounted(async () => {
    document.documentElement.classList.add('osd-page');
    unlisten = await listen<VolumeShown>('osd-show', (event) => {
      shown.value = event.payload;
      pulse.value += 1;
    });
  });

  onUnmounted(() => unlisten?.());

  const target = computed(
    () => shown.value?.levels.find((level) => level.key === shown.value?.key) ?? null
  );
  const strip = computed(() => {
    const levels = shown.value?.levels ?? [];
    const at = levels.findIndex((level) => level.key === shown.value?.key);
    const start = Math.max(0, Math.min(at - Math.floor(STRIP / 2), levels.length - STRIP));
    return levels.slice(start, start + STRIP);
  });
  const percent = computed(() => Math.round((target.value?.volume ?? 0) * 100));
  const subtitle = computed(() => {
    if (!target.value) return '';
    if (target.value.key === 'master') return 'Master · Alt+volume picks an app';
    return target.value.muted ? 'Muted' : 'Alt+volume for the next app';
  });

  function wheel(event: WheelEvent) {
    event.preventDefault();
    void osdWheel(event.deltaY < 0 ? 0.02 : -0.02);
  }
</script>

<template>
  <div
    v-if="target"
    :key="pulse"
    class="osd"
    :class="{ muted: target.muted }"
    role="status"
    :aria-label="`${target.name} ${target.muted ? 'muted' : `${percent}%`}`"
    @wheel="wheel"
    @click="osdOpenSound"
  >
    <div class="osd-top">
      <SpeakerIcon
        v-if="target.key === 'master'"
        :volume="target.volume"
        :muted="target.muted"
        :size="22"
      />
      <RowIcon v-else :icon="target.icon" :label="target.name" :size="20" />
      <span class="osd-name">
        {{ target.name }}
        <small>{{ subtitle }}</small>
      </span>
      <span class="osd-value">
        <svg
          v-if="target.muted"
          viewBox="0 0 24 24"
          aria-hidden="true"
          v-html="GLYPHS.speakerMuted"
        />
        <template v-else>{{ percent }}</template>
      </span>
    </div>
    <div class="osd-bar"><span :style="{ width: `${percent}%` }" /></div>
    <div v-if="strip.length > 1" class="osd-strip">
      <span
        v-for="level in strip"
        :key="level.key"
        class="osd-app"
        :class="{ on: level.key === target.key, quiet: !level.active }"
      >
        <SpeakerIcon
          v-if="level.key === 'master'"
          :volume="level.volume"
          :muted="level.muted"
          :size="16"
        />
        <RowIcon v-else :icon="level.icon" :label="level.name" :size="16" />
      </span>
    </div>
  </div>
</template>
