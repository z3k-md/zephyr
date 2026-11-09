<script setup lang="ts">
  import RowIcon from './RowIcon.vue';
  import SpeakerIcon from './SpeakerIcon.vue';
  import { GLYPHS } from './glyphs';
  import { vTip } from '../tip';
  import { percent, type MixerLevel } from './mixer';

  // One level in the mixer, in the Sound view and the bar's drawer alike: the icon, the name
  // (with whatever the view puts under it), a slider to press or drag, the percent and mute.
  defineProps<{ row: MixerLevel; selected: boolean; master: boolean }>();
  defineEmits<{ slide: [PointerEvent]; wheel: [WheelEvent]; mute: []; hover: [] }>();
</script>

<template>
  <div
    class="sound-row"
    :class="{ selected, muted: row.muted, master }"
    role="option"
    :aria-selected="selected"
    :aria-label="`${row.name}, ${row.muted ? 'muted' : `${percent(row.volume)}%`}`"
    @mousemove="$emit('hover')"
    @wheel="$emit('wheel', $event)"
  >
    <SpeakerIcon v-if="row.key === 'master'" :volume="row.volume" :muted="row.muted" />
    <RowIcon v-else :icon="row.icon" :label="row.name" :size="20" />
    <span class="sound-name">
      {{ row.name }}
      <slot />
    </span>
    <span
      class="sound-slider"
      role="slider"
      :aria-valuenow="percent(row.volume)"
      aria-valuemin="0"
      aria-valuemax="100"
      @mousedown.prevent
      @pointerdown.prevent="$emit('slide', $event)"
    >
      <span class="sound-track">
        <span
          v-if="row.peak !== undefined"
          class="sound-peak"
          :style="{ width: `${percent(row.muted ? 0 : row.peak)}%` }"
        />
      </span>
      <span class="sound-fill" :style="{ width: `${percent(row.volume)}%` }" />
      <span class="sound-knob" :style="{ left: `${percent(row.volume)}%` }" />
    </span>
    <span class="sound-pct">{{ percent(row.volume) }}%</span>
    <button
      type="button"
      class="sound-mute"
      :class="{ on: row.muted }"
      :aria-label="row.muted ? `Unmute ${row.name}` : `Mute ${row.name}`"
      v-tip="row.muted ? 'Unmute (M)' : 'Mute (M)'"
      @mousedown.prevent
      @click="$emit('mute')"
    >
      <svg
        viewBox="0 0 24 24"
        aria-hidden="true"
        v-html="row.muted ? GLYPHS.speakerMuted : GLYPHS.speaker"
      />
    </button>
  </div>
</template>
