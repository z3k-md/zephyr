<script setup lang="ts">
  import { computed } from 'vue';
  import type { Forecast } from '../types';
  import { GLYPHS } from './glyphs';
  import { vTip } from '../tip';
  import { condition, degrees, weatherGlyph, weatherTone, wetNote } from './weather';

  // The weather at the right of the bar's footer: icon, temperature and condition, or the
  // rain or snow coming instead of the condition. A click opens the weather card. The
  // condition is what gives way when the footer runs short.
  const props = defineProps<{ forecast: Forecast }>();
  const emit = defineEmits<{ open: [] }>();

  const wet = computed(() => wetNote(props.forecast));
</script>

<template>
  <button
    v-tip="'Weather'"
    type="button"
    class="footer-weather"
    @mousedown.prevent
    @click.stop="emit('open')"
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
</template>
