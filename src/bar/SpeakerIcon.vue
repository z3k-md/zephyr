<script setup lang="ts">
  // A speaker that shows its level, as Maestro drew it: struck through when muted, no waves
  // at zero, one wave under half, two from half up.
  defineProps<{ volume: number; muted: boolean; size?: number }>();
</script>

<template>
  <svg
    class="speaker-icon"
    viewBox="0 0 24 24"
    :width="size ?? 20"
    :height="size ?? 20"
    fill="none"
    stroke="currentColor"
    stroke-width="1.8"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    <path d="M11 5 6 9H2v6h4l5 4z" />
    <path v-if="muted" d="m23 9-6 6M17 9l6 6" />
    <template v-else>
      <path v-if="volume > 0" d="M15.54 8.46a5 5 0 0 1 0 7.07" />
      <path v-if="volume >= 0.5" d="M19.07 4.93a10 10 0 0 1 0 14.14" />
    </template>
  </svg>
</template>
