<script setup lang="ts">
  import { computed } from 'vue';
  import { shortcutKeys, spokenKeys } from '../keys';

  // A key or combo as keycaps: "mod+enter" shows ⌘ ↵ on a Mac and Ctrl ↵ elsewhere. A list
  // is taken as keycaps already named (from shortcutKeys).
  const props = defineProps<{ keys: string | string[]; small?: boolean }>();

  const caps = computed(() => (Array.isArray(props.keys) ? props.keys : shortcutKeys(props.keys)));
</script>

<template>
  <span class="keys" :class="{ small }">
    <kbd v-for="(key, index) in caps" :key="index" class="kbd" aria-hidden="true">{{ key }}</kbd>
    <span class="keys-spoken">{{ spokenKeys(caps) }}</span>
  </span>
</template>
