<script setup lang="ts">
  import { computed } from 'vue';
  import { timeLeft } from './timers';

  // The running focus session, beside what's playing: a dot, its name, the time left, and
  // how far through it. The bar passes the clock, which moves only while the bar shows.
  const props = defineProps<{
    label: string;
    seconds: number;
    endsAt: number;
    now: number;
  }>();

  const left = computed(() => Math.max(0, props.endsAt - props.now / 1000));
  const text = computed(() => timeLeft(left.value));
  const progress = computed(() => {
    if (props.seconds <= 0) return 0;
    return Math.min(1, Math.max(0, 1 - left.value / props.seconds));
  });
  const name = computed(() => props.label || 'Focus');
</script>

<template>
  <span class="focus-strip" :aria-label="`${name}, ${text} left`">
    <span class="focus-strip-dot" aria-hidden="true" />
    <span class="focus-strip-label">{{ name }}</span>
    <span class="focus-strip-time">{{ text }}</span>
    <span class="focus-strip-bar" aria-hidden="true">
      <span class="focus-strip-fill" :style="{ transform: `scaleX(${progress})` }" />
    </span>
  </span>
</template>
