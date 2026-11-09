<script setup lang="ts">
  import { computed } from 'vue';
  import { withBase } from 'vitepress';
  import { data as release } from '../release.data';

  const installHref = withBase('/guide/install');
  const releasesFallback = 'https://github.com/z3k-md/zephyr/releases/latest';

  const versionLabel = computed(() => (release.version ? `v${release.version}` : 'Latest release'));

  const windowsHref = computed(() => release.windows ?? release.url ?? releasesFallback);
  const macosHref = computed(() => release.macos ?? release.url ?? releasesFallback);
</script>

<template>
  <form class="downloads" @submit.prevent>
    <div class="downloads-buttons">
      <a class="VPButton medium brand" :href="windowsHref" target="_blank" rel="noreferrer">
        Windows
      </a>
      <a class="VPButton medium brand" :href="macosHref" target="_blank" rel="noreferrer">
        macOS (Apple silicon)
      </a>
    </div>
    <p class="downloads-meta">
      <span>{{ versionLabel }}</span>
      <span aria-hidden="true"> · </span>
      <a :href="installHref">Install guide</a>
    </p>
  </form>
</template>

<style scoped>
  .downloads {
    margin: 1.5rem 0 2rem;
    text-align: center;
  }

  .downloads-buttons {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 0.75rem;
  }

  .downloads-meta {
    margin: 0.75rem 0 0;
    font-size: 0.875rem;
    color: var(--vp-c-text-2);
  }
</style>
