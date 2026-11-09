<script setup lang="ts">
  import { computed } from 'vue';
  import { useData, withBase } from 'vitepress';

  const { frontmatter } = useData();

  const gaps = computed(() => {
    const list = frontmatter.value.gaps;
    return Array.isArray(list) ? list.map(String) : [];
  });

  const roadmapHref = withBase('/roadmap');
</script>

<template>
  <div v-if="gaps.length > 0" class="vp-doc feature-not-yet">
    <div class="custom-block info">
      <p class="custom-block-title">Not yet</p>
      <ul>
        <li v-for="(gap, index) in gaps" :key="index">{{ gap }}</li>
      </ul>
      <p>See the <a :href="roadmapHref">roadmap</a> for what is still being built.</p>
    </div>
  </div>
</template>
