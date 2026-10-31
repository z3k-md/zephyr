<script setup lang="ts">
  import { nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
  import { vTip } from '../tip';

  // One line of text that scrolls when it's too long to fit: it rests for a moment, glides
  // left at a steady pace and comes round again. Text that fits, or reduced motion, stays put
  // with an ellipsis.
  const props = defineProps<{ text: string }>();

  /** Pixels a second. */
  const SPEED = 30;
  const REST_MS = 2000;
  /** Space between the end of the text and its next lap, in px (matches .marquee-copy). */
  const GAP = 40;
  /** Width of the left fade while gliding, in px, and how long it takes to ease in or out. */
  const FADE = 6;
  const FADE_MS = 300;

  const box = ref<HTMLElement | null>(null);
  const track = ref<HTMLElement | null>(null);
  const first = ref<HTMLElement | null>(null);
  const moving = ref(false);
  const still = window.matchMedia('(prefers-reduced-motion: reduce)');
  let lap: Animation | null = null;
  let fade: Animation | null = null;
  let observer: ResizeObserver | null = null;

  async function update() {
    lap?.cancel();
    fade?.cancel();
    lap = fade = null;
    const width = first.value?.getBoundingClientRect().width ?? 0;
    moving.value = !still.matches && !!box.value && width > box.value.clientWidth + 1;
    if (!moving.value) return;
    await nextTick();
    if (!track.value || !box.value) return;
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
    // The left edge only fades while the text glides, easing back out as the lap returns to rest.
    const rest = REST_MS / duration;
    const ease = FADE_MS / duration;
    fade = box.value.animate(
      [
        { '--marquee-fade-left': '0px', offset: 0 },
        { '--marquee-fade-left': '0px', offset: rest },
        { '--marquee-fade-left': `${FADE}px`, offset: rest + ease },
        { '--marquee-fade-left': `${FADE}px`, offset: 1 - ease },
        { '--marquee-fade-left': '0px', offset: 1 },
      ],
      { duration, iterations: Infinity }
    );
  }

  function restart() {
    if (document.visibilityState === 'visible') void update();
  }

  /** The bar's webview still counts as visible once hidden, so the lap would keep painting. */
  function pause() {
    lap?.pause();
    fade?.pause();
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
    // The lap stops when the bar loses focus (it's hidden); each time it's back (the bar
    // summoned), the text starts over from rest rather than mid-scroll.
    window.addEventListener('blur', pause);
    window.addEventListener('focus', restart);
    document.addEventListener('visibilitychange', restart);
  });

  onUnmounted(() => {
    lap?.cancel();
    fade?.cancel();
    observer?.disconnect();
    still.removeEventListener('change', update);
    window.removeEventListener('blur', pause);
    window.removeEventListener('focus', restart);
    document.removeEventListener('visibilitychange', restart);
  });
</script>

<template>
  <span ref="box" v-tip="moving ? text : undefined" class="marquee" :class="{ moving }">
    <span ref="track" class="marquee-track"
      ><span ref="first">{{ text }}</span
      ><span v-if="moving" class="marquee-copy" aria-hidden="true">{{ text }}</span></span
    >
  </span>
</template>
