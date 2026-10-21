<script setup lang="ts">
  import { defineAsyncComponent } from 'vue';

  // Each window loads only its own view: Settings never pulls in the bar or the notes
  // editor, which made it slow to open.
  const params = new URLSearchParams(window.location.search);
  const view = params.get('view');
  const isSettings = view === 'settings' || window.location.hash.startsWith('#settings');

  const Page = defineAsyncComponent(() => {
    if (view === 'grid') return import('./grid/GridView.vue');
    if (view === 'update') return import('./update/UpdateView.vue');
    if (view === 'notes') return import('./notes/NotesView.vue');
    if (view === 'onboarding') return import('./onboarding/OnboardingView.vue');
    if (isSettings) return import('./settings/SettingsView.vue');
    return import('./bar/BarView.vue');
  });
</script>

<template>
  <Page />
</template>
