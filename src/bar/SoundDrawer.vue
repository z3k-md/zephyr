<script setup lang="ts">
  import { computed } from 'vue';
  import RowIcon from './RowIcon.vue';
  import SpeakerIcon from './SpeakerIcon.vue';
  import { GLYPHS } from './glyphs';
  import type { VolumeShown } from '../types';

  // The volume keys while the bar is open: a few levels slide up over the footer instead of
  // the overlay, the keys' target lit. The bar hides it a moment after the last press.
  const props = defineProps<{ shown: VolumeShown }>();
  defineEmits<{ open: [] }>();

  /** Master, the target, and what's playing, up to this many rows. */
  const ROWS = 4;

  const rows = computed(() => {
    const { key, levels } = props.shown;
    const picked = levels.filter(
      (level, index) => index === 0 || level.key === key || level.active
    );
    // The target always makes the cut.
    const kept = picked.slice(0, ROWS);
    const target = picked.find((level) => level.key === key);
    if (target && !kept.includes(target)) kept[ROWS - 1] = target;
    return kept;
  });

  function percent(value: number): number {
    return Math.round(value * 100);
  }
</script>

<template>
  <section class="sound-drawer" aria-label="Volume" @mousedown.prevent @click="$emit('open')">
    <div
      v-for="level in rows"
      :key="level.key"
      class="sound-row"
      :class="{ selected: level.key === shown.key, muted: level.muted }"
    >
      <SpeakerIcon v-if="level.key === 'master'" :volume="level.volume" :muted="level.muted" />
      <RowIcon v-else :icon="level.icon" :label="level.name" :size="20" />
      <span class="sound-name">{{ level.name }}</span>
      <span class="sound-slider">
        <span class="sound-track" />
        <span class="sound-fill" :style="{ width: `${percent(level.volume)}%` }" />
        <span class="sound-knob" :style="{ left: `${percent(level.volume)}%` }" />
      </span>
      <span class="sound-pct">
        <svg
          v-if="level.muted"
          viewBox="0 0 24 24"
          aria-label="Muted"
          v-html="GLYPHS.speakerMuted"
        />
        <template v-else>{{ percent(level.volume) }}</template>
      </span>
    </div>
  </section>
</template>
