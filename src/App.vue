<script setup lang="ts">
  import { defineAsyncComponent } from 'vue';

  // Each window loads only its own view: Settings never pulls in the bar or the notes
  // editor, which made it slow to open.
  const params = new URLSearchParams(window.location.search);
  const view = params.get('view');
  const isSettings = view === 'settings' || window.location.hash.startsWith('#settings');

  // Keep each import in its own loader. Combining conditional imports inside one
  // callback lets the production bundler attach only the last branch's CSS preload.
  const pages = {
    grid: () => import('./grid/GridView.vue'),
    osd: () => import('./osd/OsdView.vue'),
    update: () => import('./update/UpdateView.vue'),
    notes: () => import('./notes/NotesView.vue'),
    onboarding: () => import('./onboarding/OnboardingView.vue'),
    settings: () => import('./settings/SettingsView.vue'),
    bar: () => import('./bar/BarView.vue'),
  };
  const page = isSettings ? 'settings' : view;
  const loader =
    page && Object.prototype.hasOwnProperty.call(pages, page)
      ? pages[page as keyof typeof pages]
      : pages.bar;
  const Page = defineAsyncComponent(loader);
</script>

<template>
  <Page />
</template>
