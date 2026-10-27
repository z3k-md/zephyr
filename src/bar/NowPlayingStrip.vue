<script setup lang="ts">
  import { computed } from 'vue';
  import type { NowPlaying } from '../types';
  import { iconFor } from './appIcons';
  import { GLYPHS } from './glyphs';
  import { vTip } from '../tip';
  import Marquee from '../components/Marquee.vue';

  // What's playing, at the right of the bar's footer: the cover (or the playing app's icon),
  // the title and artist, and previous, play/pause and next. Clicking the title switches to
  // what's playing.

  const props = defineProps<{ track: NowPlaying }>();
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
  const PLAY = '<path d="M8 5.5v13l10.5-6.5z"/>';
  const PAUSE = '<path d="M8.5 5.5v13M15.5 5.5v13"/>';
  const PREVIOUS = '<path d="M6.5 5.5v13M18 5.5v13L8.5 12z"/>';
  const NEXT = '<path d="M17.5 5.5v13M6 5.5v13l9.5-6.5z"/>';
</script>

<template>
  <span class="now-playing">
    <!-- mousedown.prevent keeps the typing caret in the bar's field. -->
    <button
      type="button"
      class="now-playing-title"
      v-tip="`Switch to ${track.title}`"
      @mousedown.prevent
      @click="emit('focus')"
    >
      <img v-if="cover" class="now-playing-cover" :src="cover" alt="" />
      <svg v-else class="now-playing-cover glyph" viewBox="0 0 24 24" aria-hidden="true">
        <g v-html="GLYPHS.music" />
      </svg>
      <span class="now-playing-text">
        <Marquee class="now-playing-song" :text="track.title" />
        <span v-if="track.artist" class="now-playing-artist">{{ track.artist }}</span>
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
