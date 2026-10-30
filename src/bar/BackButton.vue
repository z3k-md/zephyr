<script setup lang="ts">
  import { GLYPHS } from './glyphs';

  // Where a view's mark would be: back to the main bar, for the mouse (Esc does it by key).
  defineEmits<{ back: [] }>();
</script>

<template>
  <button
    type="button"
    class="back-button"
    aria-label="Back"
    v-tip="'Back (Esc)'"
    @mousedown.prevent
    @click="$emit('back')"
  >
    <svg class="mark" viewBox="0 0 24 24" aria-hidden="true" v-html="GLYPHS.back" />
  </button>
</template>

<style scoped>
  .back-button {
    display: grid;
    place-items: center;
    flex: none;
    width: 26px;
    height: 26px;
    margin: -4px;
    padding: 0;
    border: 0;
    border-radius: 6px;
    background: transparent;
    cursor: pointer;
  }

  .back-button:hover {
    background: var(--chip-hover);
  }

  .back-button:hover .mark {
    stroke: var(--fg);
  }
</style>
