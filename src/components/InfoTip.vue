<script setup lang="ts">
  import { GLYPHS } from '../bar/glyphs';
  import { vTip } from '../tip';

  // A small ⓘ beside a setting's label or a page's heading; what it explains shows in the
  // app's own tooltip on hover, so the page carries no paragraphs of help.
  defineProps<{ text: string }>();
</script>

<template>
  <span v-tip="text" class="info-dot" role="img" :aria-label="text">
    <svg viewBox="0 0 24 24" aria-hidden="true" v-html="GLYPHS.info" />
  </span>
</template>
