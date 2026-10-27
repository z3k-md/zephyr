<script setup lang="ts">
  import { nextTick, onMounted, onUnmounted, ref, watch } from 'vue';

  // One line of text that scrolls when it's too long to fit: it rests for a moment, glides
  // left at a steady pace and comes round again. Text that fits, or reduced motion, stays put
  // with an ellipsis.
  const props = defineProps<{ text: string }>();

  /** Pixels a second. */
  const SPEED = 30;
  const REST_MS = 2000;
  /** Space between the end of the text and its next lap, in px (matches .marquee-copy). */
  const GAP = 40;

  const box = ref<HTMLElement | null>(null);
  const track = ref<HTMLElement | null>(null);
  const first = ref<HTMLElement | null>(null);
  const moving = ref(false);
  const still = window.matchMedia('(prefers-reduced-motion: reduce)');
  let lap: Animation | null = null;
  let observer: ResizeObserver | null = null;

  async function update() {
    lap?.cancel();
    lap = null;
    const width = first.value?.getBoundingClientRect().width ?? 0;
    moving.value = !still.matches && !!box.value && width > box.value.clientWidth + 1;
    if (!moving.value) return;
    await nextTick();
    if (!track.value) return;
    // Two copies of the text, so the lap ends exactly where it began.
    const distance = width + GAP;
    const duration = REST_MS + (distance / SPEED) * 1000;
    lap = track.value.animate(
      [
        { transform: 'translateX(0)', offset: 0 },
        { transform: 'translateX(0)', offset: REST_MS / duration },
        { transform: `translateX(-${distance}px)`, offset: 1 },
      ],
      { duration, iterations: Infinity }
    );
  }

  watch(
    () => props.text,
    () => void nextTick(update)
  );

  onMounted(() => {
    void update();
    observer = new ResizeObserver(() => void update());
    if (box.value) observer.observe(box.value);
    still.addEventListener('change', update);
  });

  onUnmounted(() => {
    lap?.cancel();
    observer?.disconnect();
    still.removeEventListener('change', update);
  });
</script>

<template>
  <span ref="box" class="marquee" :class="{ moving }" :title="moving ? text : undefined">
    <span ref="track" class="marquee-track"
      ><span ref="first">{{ text }}</span
      ><span v-if="moving" class="marquee-copy" aria-hidden="true">{{ text }}</span></span
    >
  </span>
</template>
