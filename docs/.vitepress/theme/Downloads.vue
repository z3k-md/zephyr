<script setup lang="ts">
  import { computed } from 'vue';
  import { withBase } from 'vitepress';
  import { data as release } from '../release.data';

  const installHref = withBase('/guide/install');
  const windowsHref = computed(() => release.windows ?? release.url);
  const macosHref = computed(() => release.macos ?? release.url);
</script>

<template>
  <div class="downloads">
    <div class="downloads-buttons">
      <a class="downloads-button" :href="windowsHref">Windows</a>
      <a class="downloads-button" :href="macosHref">macOS (Apple silicon)</a>
    </div>
    <p class="downloads-meta">
      <span v-if="release.version">v{{ release.version }} · </span>
      <a :href="installHref">First launch</a>
    </p>
  </div>
</template>

<style scoped>
  .downloads {
    padding-top: 24px;
  }

  .downloads-buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
  }

  .downloads-button {
    display: inline-block;
    border: 1px solid var(--vp-button-brand-border);
    border-radius: 20px;
    padding: 0 20px;
    line-height: 38px;
    font-size: 14px;
    font-weight: 600;
    color: var(--vp-button-brand-text);
    background-color: var(--vp-button-brand-bg);
    text-decoration: none;
    transition: background-color 0.25s;
  }

  .downloads-button:hover {
    border-color: var(--vp-button-brand-hover-border);
    color: var(--vp-button-brand-hover-text);
    background-color: var(--vp-button-brand-hover-bg);
  }

  .downloads-meta {
    margin: 12px 0 0;
    font-size: 14px;
    color: var(--vp-c-text-2);
  }

  .downloads-meta a {
    color: var(--vp-c-brand-1);
  }

  @media (max-width: 959px) {
    .downloads-buttons {
      justify-content: center;
    }

    .downloads-meta {
      text-align: center;
    }
  }
</style>
