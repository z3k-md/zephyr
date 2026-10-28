<script setup lang="ts">
  import './grid.css';
  import { computed, onMounted, onUnmounted, ref } from 'vue';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { useWindowListener } from '../windowEvents';

  // The snap grid over the screen while the bar is Ctrl+dragged: the guides the bar's center
  // can snap to (thirds and the middle, both ways), the line its top edge snaps to when it
  // opens by default, and an outline where the bar lands if dropped now.

  interface Preview {
    x: number;
    y: number;
    width: number;
    height: number;
  }

  const width = ref(window.innerWidth);
  const height = ref(window.innerHeight);
  const preview = ref<Preview | null>(null);
  let unlisten: UnlistenFn | null = null;

  function resize() {
    width.value = window.innerWidth;
    height.value = window.innerHeight;
  }

  useWindowListener('resize', resize);

  onMounted(async () => {
    document.documentElement.classList.add('grid-page');
    unlisten = await listen<Preview | null>('grid-preview', (event) => {
      // Rust sends physical pixels; the page works in CSS pixels.
      const scale = window.devicePixelRatio || 1;
      preview.value = event.payload
        ? {
            x: event.payload.x / scale,
            y: event.payload.y / scale,
            width: event.payload.width / scale,
            height: event.payload.height / scale,
          }
        : null;
    });
  });

  onUnmounted(() => unlisten?.());

  const across = computed(() => [1 / 3, 1 / 2, 2 / 3].map((part) => width.value * part));
  const down = computed(() => [1 / 3, 1 / 2, 2 / 3].map((part) => height.value * part));
  const topLine = computed(() => height.value / 6);

  /** A guide the outline sits on lights up. */
  const lit = computed(() => {
    const box = preview.value;
    if (!box) return { x: -1, y: -1, top: false };
    const near = (a: number, b: number) => Math.abs(a - b) < 2;
    const centerX = box.x + box.width / 2;
    const centerY = box.y + box.height / 2;
    return {
      x: across.value.findIndex((line) => near(line, centerX)),
      y: down.value.findIndex((line) => near(line, centerY)),
      top: near(box.y, topLine.value),
    };
  });
</script>

<template>
  <svg class="grid" :viewBox="`0 0 ${width} ${height}`" aria-hidden="true">
    <line
      v-for="(x, index) in across"
      :key="`x${index}`"
      :x1="x"
      :x2="x"
      y1="0"
      :y2="height"
      :class="{ lit: lit.x === index }"
    />
    <line
      v-for="(y, index) in down"
      :key="`y${index}`"
      x1="0"
      :x2="width"
      :y1="y"
      :y2="y"
      :class="{ lit: lit.y === index }"
    />
    <line class="top" x1="0" :x2="width" :y1="topLine" :y2="topLine" :class="{ lit: lit.top }" />
    <rect
      v-if="preview"
      class="landing"
      :x="preview.x"
      :y="preview.y"
      :width="preview.width"
      :height="preview.height"
      rx="10"
    />
  </svg>
</template>
