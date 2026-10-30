<script setup lang="ts">
  import { computed } from 'vue';
  import type { Forecast } from '../types';
  import KeyHints from '../components/KeyHints.vue';
  import { GLYPHS } from './glyphs';
  import {
    condition,
    degrees,
    hourLabel,
    placeName,
    weatherGlyph,
    weatherTone,
    weekday,
    windSpeed,
  } from './weather';

  // The weather card over the results for "weather", "forecast" or "weather <city>": now,
  // eight hours with their temperatures as a line, and four days as ranges. Tab (or hovering
  // an hour) steps through the next 48 hours, eight at a time, showing that hour's sky and
  // chance of rain; Shift+Tab back past the first returns to now. Enter stays on the search
  // row; Ctrl+Enter opens a full forecast.
  const props = defineProps<{ forecast: Forecast; hour: number | null }>();
  defineEmits<{ open: []; hour: [hour: number] }>();

  const start = computed(() => (props.hour === null ? 0 : Math.floor(props.hour / 8) * 8));
  const hours = computed(() => props.forecast.hourly.slice(start.value, start.value + 8));
  const picked = computed(() =>
    props.hour === null ? null : (props.forecast.hourly[props.hour] ?? null)
  );
  const days = computed(() => props.forecast.daily.slice(0, 4));
  const uv = computed(() => Math.round(props.forecast.daily[0]?.uv ?? 0));

  // The hours' temperatures as a line across the strip, each point over its column.
  const line = computed(() => {
    const temps = hours.value.map((hour) => hour.temperature);
    if (temps.length < 2) return '';
    const low = Math.min(...temps);
    const span = Math.max(...temps) - low || 1;
    const step = 100 / temps.length;
    return temps
      .map((temp, index) => `${step * (index + 0.5)},${18 - ((temp - low) / span) * 14}`)
      .join(' ');
  });

  // Each day's range on one scale, from the four days' lowest to highest.
  const scale = computed(() => {
    const low = Math.min(...days.value.map((day) => day.low));
    const high = Math.max(...days.value.map((day) => day.high));
    return { low, span: high - low || 1 };
  });
  function rangeStyle(low: number, high: number) {
    const { low: floor, span } = scale.value;
    return {
      left: `${((low - floor) / span) * 100}%`,
      width: `${Math.max(6, ((high - low) / span) * 100)}%`,
    };
  }
</script>

<template>
  <section class="weather-card" aria-label="Weather" @mousedown.prevent>
    <div class="weather-now">
      <div class="weather-head">
        <span class="weather-place">{{ placeName(forecast) }}</span>
        <KeyHints
          :hints="[
            { keys: 'tab', label: 'Hours' },
            { keys: 'mod+enter', label: 'Forecast' },
          ]"
          @click="$emit('open')"
        />
      </div>
      <div v-if="picked" class="weather-main">
        <svg
          class="weather-big"
          :class="weatherTone(picked.code, picked.isDay)"
          viewBox="0 0 24 24"
          aria-hidden="true"
          v-html="GLYPHS[weatherGlyph(picked.code, picked.isDay)]"
        />
        <span class="weather-temp">{{ degrees(picked.temperature) }}</span>
        <span class="weather-detail">
          <b>{{ condition(picked.code) }}</b>
          <span
            >{{ weekday(forecast, picked.time) }} {{ hourLabel(forecast, picked.time) }} · Rain
            {{ Math.round(picked.precipitation) }}%</span
          >
        </span>
      </div>
      <div v-else class="weather-main">
        <svg
          class="weather-big"
          :class="weatherTone(forecast.current.code, forecast.current.isDay)"
          viewBox="0 0 24 24"
          aria-hidden="true"
          v-html="GLYPHS[weatherGlyph(forecast.current.code, forecast.current.isDay)]"
        />
        <span class="weather-temp">{{ degrees(forecast.current.temperature) }}</span>
        <span class="weather-detail">
          <b>{{ condition(forecast.current.code) }}</b>
          <span
            >Feels {{ degrees(forecast.current.feelsLike) }} · Wind
            {{ windSpeed(forecast.current.wind) }} · UV {{ uv }}</span
          >
        </span>
      </div>
      <div class="weather-hours">
        <svg
          class="weather-line"
          viewBox="0 0 100 20"
          preserveAspectRatio="none"
          aria-hidden="true"
        >
          <polyline :points="line" />
        </svg>
        <span
          v-for="(hour, index) in hours"
          :key="hour.time"
          class="weather-hour"
          :class="{ on: start + index === props.hour }"
          @mouseenter="$emit('hour', start + index)"
        >
          <span class="weather-hour-temp">{{ degrees(hour.temperature) }}</span>
          <svg
            :class="weatherTone(hour.code, hour.isDay)"
            viewBox="0 0 24 24"
            aria-hidden="true"
            v-html="GLYPHS[weatherGlyph(hour.code, hour.isDay)]"
          />
          <small>{{ hourLabel(forecast, hour.time) }}</small>
        </span>
      </div>
    </div>
    <ul class="weather-days">
      <li v-for="day in days" :key="day.time">
        <span class="weather-day">{{ weekday(forecast, day.time) }}</span>
        <svg
          :class="weatherTone(day.code)"
          viewBox="0 0 24 24"
          aria-hidden="true"
          v-html="GLYPHS[weatherGlyph(day.code)]"
        />
        <span class="weather-low">{{ degrees(day.low) }}</span>
        <span class="weather-range" aria-hidden="true">
          <span :style="rangeStyle(day.low, day.high)" />
        </span>
        <span class="weather-high">{{ degrees(day.high) }}</span>
      </li>
    </ul>
  </section>
</template>
