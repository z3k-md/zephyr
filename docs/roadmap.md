---
title: Roadmap
---

<script setup>
import { data } from './.vitepress/roadmap.data'
</script>

Bugs belong in [GitHub Issues](https://github.com/z3k-md/zephyr/issues). This page lists known gaps and future work from the feature pages; there are no dates on purpose.

## In progress

<p v-if="!data.inProgress.length">Nothing is marked in progress yet.</p>
<ul v-else>
  <li v-for="item in data.inProgress" :key="item.url">
    <a :href="item.url">{{ item.title }}</a>
    <span v-if="item.summary"> — {{ item.summary }}</span>
    <ul v-if="item.gaps.length">
      <li v-for="(gap, i) in item.gaps" :key="i">{{ gap }}</li>
    </ul>
  </li>
</ul>

## Planned

<p v-if="!data.planned.length">No planned features are listed yet.</p>
<ul v-else>
  <li v-for="item in data.planned" :key="item.url">
    <a :href="item.url">{{ item.title }}</a>
    <span v-if="item.summary"> — {{ item.summary }}</span>
  </li>
</ul>

## Gaps in shipped features

<p v-if="!data.gaps.length">No documented gaps yet.</p>
<ul v-else>
  <li v-for="(gap, i) in data.gaps" :key="i">
    <a :href="gap.pageUrl">{{ gap.pageTitle }}</a>: {{ gap.text }}
  </li>
</ul>
