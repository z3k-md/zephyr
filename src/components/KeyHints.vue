<script setup lang="ts">
  import Keys from './Keys.vue';

  // A row of key hints, each its keycaps and what they do: ↵ paste  ⌘↵ copy.
  defineProps<{ hints: { keys: string; label: string }[] }>();
</script>

<template>
  <span class="key-hints">
    <span v-for="hint in hints" :key="hint.keys + hint.label" class="key-hint">
      <Keys :keys="hint.keys" small />
      <span v-if="hint.label">{{ hint.label }}</span>
    </span>
  </span>
</template>
