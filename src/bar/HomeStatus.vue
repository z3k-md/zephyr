<script setup lang="ts">
  import { computed } from 'vue';
  import type { Forecast, MediaControl, NowPlaying } from '../types';
  import { iconFor } from './appIcons';
  import { GLYPHS } from './glyphs';
  import { vTip } from '../tip';
  import { condition, degrees, weatherGlyph, weatherTone, wetNote } from './weather';

  // The empty bar's footer, before the meters: what's playing and the weather, each only
  // when there's something to show. Its root is `display: contents`, so both sit in the
  // footer's own row: the title gives way first when space is short, then the weather's
  // condition; the meters never do.
  const props = defineProps<{ track: NowPlaying | null; forecast: Forecast | null }>();
  const emit = defineEmits<{ press: [control: MediaControl]; focus: []; weather: [] }>();

  // A browser's own icon says nothing about the song, so a tab with no cover gets the note.
  const cover = computed(() => {
    const track = props.track;
    if (!track) return null;
    return track.artwork ?? (isBrowser(track.sourceApp) ? null : iconFor(track.sourceApp, 18));
  });

  function isBrowser(app: string) {
    return /^(chrome|chromium|msedge|brave|vivaldi|opera|arc)\b/i.test(app);
  }

  const song = computed(() =>
    props.track ? [props.track.title, props.track.artist].filter(Boolean).join(' · ') : ''
  );
  /** Rain or snow coming takes the condition's place. */
  const wet = computed(() => (props.forecast ? wetNote(props.forecast) : null));

  const PLAY = '<path d="M8 5.5v13l10.5-6.5z"/>';
  const PAUSE = '<path d="M8.5 5.5v13M15.5 5.5v13"/>';
  const NEXT = '<path d="M17.5 5.5v13M6 5.5v13l9.5-6.5z"/>';
</script>

<template>
  <span class="home-status">
    <span v-if="track" class="status-track">
      <!-- mousedown.prevent keeps the typing caret in the bar's field. -->
      <button
        type="button"
        class="status-song"
        v-tip="`${song}\nClick to switch to it`"
        @mousedown.prevent
        @click.stop="emit('focus')"
      >
        <img v-if="cover" class="status-cover" :src="cover" alt="" />
        <svg v-else class="status-cover glyph" viewBox="0 0 24 24" aria-hidden="true">
          <g v-html="GLYPHS.music" />
        </svg>
        <span class="status-song-text">{{ song }}</span>
      </button>
      <button
        type="button"
        class="status-control"
        :aria-label="track.playing ? 'Pause' : 'Play'"
        v-tip="`${track.playing ? 'Pause' : 'Play'} (Ctrl+Space)`"
        @mousedown.prevent
        @click.stop="emit('press', 'toggle')"
      >
        <svg viewBox="0 0 24 24" aria-hidden="true" v-html="track.playing ? PAUSE : PLAY" />
      </button>
      <button
        type="button"
        class="status-control"
        aria-label="Next"
        v-tip="'Next (Ctrl+Shift+→)'"
        @mousedown.prevent
        @click.stop="emit('press', 'next')"
      >
        <svg viewBox="0 0 24 24" aria-hidden="true" v-html="NEXT" />
      </button>
    </span>
    <span v-if="track && forecast" class="status-divider" aria-hidden="true" />
    <button
      v-if="forecast"
      type="button"
      class="status-weather"
      v-tip="'Weather'"
      @mousedown.prevent
      @click.stop="emit('weather')"
    >
      <svg
        class="status-icon"
        :class="weatherTone(forecast.current.code, forecast.current.isDay)"
        viewBox="0 0 24 24"
        aria-hidden="true"
        v-html="GLYPHS[weatherGlyph(forecast.current.code, forecast.current.isDay)]"
      />
      <b>{{ degrees(forecast.current.temperature) }}</b>
      <span class="status-condition" :class="{ wet }">{{
        wet ?? condition(forecast.current.code)
      }}</span>
    </button>
  </span>
</template>
