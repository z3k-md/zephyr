<script setup lang="ts">
  import { computed } from 'vue';
  import { useData } from 'vitepress';
  import { VPBadge } from 'vitepress/theme';

  const { frontmatter } = useData();

  const show = computed(() => Boolean(frontmatter.value.status));

  const statusLabel = computed(() => {
    const status = frontmatter.value.status;
    if (status === 'shipped') return 'Shipped';
    if (status === 'partial') return 'Partly done';
    if (status === 'in-progress') return 'In progress';
    if (status === 'planned') return 'Planned';
    return '';
  });

  const statusType = computed(() => {
    const status = frontmatter.value.status;
    if (status === 'shipped') return 'tip';
    if (status === 'partial') return 'warning';
    if (status === 'in-progress') return 'warning';
    return 'info';
  });

  const platformLabels = computed(() => {
    const platforms = frontmatter.value.platforms;
    if (!Array.isArray(platforms)) return [] as string[];
    const names: Record<string, string> = {
      windows: 'Windows',
      macos: 'macOS',
      linux: 'Linux',
    };
    return platforms.map((p) => names[String(p)]).filter(Boolean);
  });

  const sinceHref = computed(() => {
    const since = frontmatter.value.since;
    if (!since) return '';
    const version = String(since).replace(/^v/, '');
    return `https://github.com/z3k-md/zephyr/releases/tag/v${version}`;
  });
</script>

<template>
  <div v-if="show" class="vp-doc feature-meta" style="margin-bottom: 1rem">
    <h1>{{ frontmatter.title }}</h1>
    <p v-if="frontmatter.summary" class="feature-meta-summary">
      {{ frontmatter.summary }}
    </p>
    <p class="feature-meta-badges">
      <VPBadge v-if="statusLabel" :text="statusLabel" :type="statusType" />
      <template v-for="label in platformLabels" :key="label">
        <VPBadge :text="label" type="info" />
      </template>
      <span v-if="frontmatter.since" class="feature-meta-since">
        Since
        <a :href="sinceHref" target="_blank" rel="noreferrer">{{ frontmatter.since }}</a>
      </span>
    </p>
  </div>
</template>

<style scoped>
  .feature-meta-summary {
    font-size: 1.125rem;
    color: var(--vp-c-text-2);
    margin: 0.5rem 0 1rem;
  }

  .feature-meta-badges {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
    margin: 0 0 1.5rem;
  }

  .feature-meta-since {
    font-size: 0.875rem;
    color: var(--vp-c-text-2);
  }
</style>
