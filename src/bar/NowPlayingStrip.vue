<script setup lang="ts">
  import { computed, onUnmounted, ref, watch } from 'vue';
  import { nowPlayingVolume } from '../api';
  import type { NowPlaying } from '../types';
  import { iconFor } from './appIcons';
  import { GLYPHS } from './glyphs';
  import { vTip } from '../tip';
  import Marquee from '../components/Marquee.vue';

  // What's playing, at the left of the footer: one card with the cover (or the playing app's
  // icon), the title, the artist and how far in, and previous, play/pause and next, under one
  // hover highlight, with the song's progress under the text: a wave while it plays, flat
  // while paused.
  // Right-clicking the card switches to what's playing; the scroll wheel over it turns that
  // app up or down (its volume mixer level on Windows, Spotify's or Music's own volume on
  // macOS), and the level shows in place of the artist.

  const props = defineProps<{ track: NowPlaying; positionMs: number | null }>();
  const emit = defineEmits<{ toggle: []; next: []; previous: []; focus: [] }>();

  // A browser's own icon says nothing about the song, so a tab with no cover gets the note.
  const cover = computed(
    () =>
      props.track.artwork ??
      (isBrowser(props.track.sourceApp) ? null : iconFor(props.track.sourceApp, 24))
  );

  function isBrowser(app: string) {
    return /^(chrome|chromium|msedge|brave|vivaldi|opera|arc)\b/i.test(app);
  }

  /** How far in, as "1:42 / 4:03"; just the position for a stream with no length. */
  const time = computed(() => {
    const at = props.positionMs;
    if (at === null) return null;
    const length = props.track.durationMs;
    return length ? `${clock(at)} / ${clock(length)}` : clock(at);
  });

  const progress = computed(() => {
    const length = props.track.durationMs;
    if (props.positionMs === null || !length) return null;
    return Math.min(1, props.positionMs / length);
  });

  // The line glides through each second's tick, but jumps straight to a position that moved
  // by more (the bar reopened after its clock stood still, a seek, a new song).
  const snap = ref(true);
  let snapFrame = 0;
  watch(
    () => props.positionMs,
    (next, before) => {
      if (next !== null && before != null && Math.abs(next - before) <= 1500) return;
      snap.value = true;
      cancelAnimationFrame(snapFrame);
      snapFrame = requestAnimationFrame(() => {
        snapFrame = requestAnimationFrame(() => (snap.value = false));
      });
    },
    { immediate: true }
  );
  onUnmounted(() => cancelAnimationFrame(snapFrame));

  function clock(ms: number) {
    const seconds = Math.floor(ms / 1000);
    const hours = Math.floor(seconds / 3600);
    const minutes = Math.floor(seconds / 60) % 60;
    const rest = String(seconds % 60).padStart(2, '0');
    return hours ? `${hours}:${String(minutes).padStart(2, '0')}:${rest}` : `${minutes}:${rest}`;
  }

  /** The level just set with the wheel, shown for a moment. */
  const level = ref<number | null>(null);
  let levelTimer = 0;
  let busy = false;
  onUnmounted(() => window.clearTimeout(levelTimer));

  // A notch is 5%; notches that land while the last change is still on its way are dropped,
  // so a fast spin doesn't queue up.
  async function onWheel(event: WheelEvent) {
    if (busy || event.deltaY === 0) return;
    busy = true;
    const next = await nowPlayingVolume(event.deltaY < 0 ? 0.05 : -0.05).catch(() => null);
    busy = false;
    if (next === null) return;
    level.value = Math.round(next * 100);
    window.clearTimeout(levelTimer);
    levelTimer = window.setTimeout(() => (level.value = null), 1200);
  }

  const PLAY = '<path d="M8 5.5v13l10.5-6.5z"/>';
  const PAUSE = '<path d="M8.5 5.5v13M15.5 5.5v13"/>';
  const PREVIOUS = '<path d="M6.5 5.5v13M18 5.5v13L8.5 12z"/>';
  const NEXT = '<path d="M17.5 5.5v13M6 5.5v13l9.5-6.5z"/>';
</script>

<template>
  <span class="now-playing" @wheel.prevent="onWheel" @contextmenu.prevent="emit('focus')">
    <!-- mousedown.prevent keeps the typing caret in the bar's field. -->
    <button
      type="button"
      class="now-playing-title"
      v-tip="`${track.playing ? 'Pause' : 'Play'} · right-click to switch to it`"
      @mousedown.prevent
      @click="emit('toggle')"
    >
      <img v-if="cover" class="now-playing-cover" :src="cover" alt="" />
      <svg v-else class="now-playing-cover glyph" viewBox="0 0 24 24" aria-hidden="true">
        <g v-html="GLYPHS.music" />
      </svg>
      <span class="now-playing-text">
        <Marquee class="now-playing-song" :text="track.title" />
        <span v-if="level !== null" class="now-playing-artist">Volume {{ level }}%</span>
        <span v-else-if="track.artist || time" class="now-playing-artist">
          {{ [track.artist, time].filter(Boolean).join(' · ') }}
        </span>
        <span
          v-if="progress !== null"
          class="now-playing-progress"
          :class="{ playing: track.playing, snap }"
          :style="{ '--progress': `${progress * 100}%` }"
          aria-hidden="true"
        >
          <span class="now-playing-played" />
          <span class="now-playing-thumb" />
        </span>
      </span>
    </button>
    <button
      type="button"
      class="now-playing-control"
      aria-label="Previous"
      v-tip="'Previous (Ctrl+Shift+←)'"
      @mousedown.prevent
      @click="emit('previous')"
    >
      <svg viewBox="0 0 24 24" aria-hidden="true" v-html="PREVIOUS" />
    </button>
    <button
      type="button"
      class="now-playing-control"
      :aria-label="track.playing ? 'Pause' : 'Play'"
      v-tip="`${track.playing ? 'Pause' : 'Play'} (Ctrl+Space)`"
      @mousedown.prevent
      @click="emit('toggle')"
    >
      <svg viewBox="0 0 24 24" aria-hidden="true" v-html="track.playing ? PAUSE : PLAY" />
    </button>
    <button
      type="button"
      class="now-playing-control"
      aria-label="Next"
      v-tip="'Next (Ctrl+Shift+→)'"
      @mousedown.prevent
      @click="emit('next')"
    >
      <svg viewBox="0 0 24 24" aria-hidden="true" v-html="NEXT" />
    </button>
  </span>
</template>
