<script setup lang="ts">
  import { computed } from 'vue';
  import BarView from './bar/BarView.vue';
  import NotesView from './notes/NotesView.vue';
  import GridView from './grid/GridView.vue';
  import UpdateView from './update/UpdateView.vue';
  import SettingsView from './settings/SettingsView.vue';

  const view = new URLSearchParams(window.location.search).get('view');

  const isSettings = computed(() => {
    const params = new URLSearchParams(window.location.search);
    return params.get('view') === 'settings' || window.location.hash.startsWith('#settings');
  });
</script>

<template>
  <GridView v-if="view === 'grid'" />
  <UpdateView v-else-if="view === 'update'" />
  <NotesView v-else-if="view === 'notes'" />
  <SettingsView v-else-if="isSettings" />
  <BarView v-else />
</template>
