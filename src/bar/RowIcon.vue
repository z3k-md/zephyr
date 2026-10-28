<script setup lang="ts">
  import { computed } from 'vue';
  import { iconFor, siteIconForUrl } from './appIcons';
  import { GLYPHS } from './glyphs';
  // The app's own icon, for rows that open its Settings window.
  import zephyrIcon from '../../src-tauri/icons/64x64.png';

  const props = withDefaults(defineProps<{ icon?: string; label: string; size?: 32 | 20 | 16 }>(), {
    icon: '',
    size: 20,
  });

  const glyph = computed(() => {
    if (props.icon.startsWith('glyph:')) return GLYPHS[props.icon.slice(6)] ?? null;
    // A Steam game whose art isn't in Steam's cache (or hasn't loaded yet).
    if (props.icon.startsWith('app:steam:')) return GLYPHS.gamepad;
    return null;
  });

  // An installed app's own icon, or a bookmarked site's; the monogram stands in until it
  // loads or if it has none.
  const image = computed(() => {
    if (props.icon === 'zephyr') return zephyrIcon;
    if (props.icon.startsWith('app:')) return iconFor(props.icon.slice(4), props.size);
    if (props.icon.startsWith('site:')) return siteIconForUrl(props.icon.slice(5));
    return null;
  });

  const letter = computed(() => (props.label.match(/[\p{L}\p{N}]/u)?.[0] ?? '·').toUpperCase());

  /** A stable hue per name, so a row keeps its color. */
  const hue = computed(() => {
    let hash = 0;
    for (const char of props.label) hash = (hash * 31 + char.charCodeAt(0)) | 0;
    return Math.abs(hash) % 360;
  });
</script>

<template>
  <span class="item-icon" :style="{ '--icon': `${size}px` }" aria-hidden="true">
    <img v-if="image" class="app-image" :src="image" alt="" />
    <svg
      v-else-if="glyph"
      class="glyph"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="1.8"
      stroke-linecap="round"
      stroke-linejoin="round"
      v-html="glyph"
    />
    <span v-else class="monogram" :style="{ background: `hsl(${hue} 40% 35% / 0.55)` }">{{
      letter
    }}</span>
  </span>
</template>
