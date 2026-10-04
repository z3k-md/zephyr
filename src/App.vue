<script setup lang="ts">
  import { computed } from 'vue';
  import BarView from './bar/BarView.vue';
  import SettingsView from './settings/SettingsView.vue';

  const isSettings = computed(() => {
    const params = new URLSearchParams(window.location.search);
    return params.get('view') === 'settings' || window.location.hash.startsWith('#settings');
  });
</script>

<template>
  <SettingsView v-if="isSettings" />
  <BarView v-else />
</template>
