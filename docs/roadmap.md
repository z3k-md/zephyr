---
title: Roadmap
---

<script setup>
import { withBase } from 'vitepress'
import { data } from './.vitepress/roadmap.data'
</script>

Built from the feature pages. No dates. Bugs go to [GitHub Issues](https://github.com/z3k-md/zephyr/issues).

## In progress

<p v-if="!data.inProgress.length">None.</p>
<ul v-else>
  <li v-for="item in data.inProgress" :key="item.url">
    <a :href="withBase(item.url)">{{ item.title }}</a><span v-if="item.summary">: {{ item.summary }}</span>
    <ul v-if="item.gaps.length">
      <li v-for="(gap, i) in item.gaps" :key="i">{{ gap }}</li>
    </ul>
  </li>
</ul>

## Planned

<p v-if="!data.planned.length">None.</p>
<ul v-else>
  <li v-for="item in data.planned" :key="item.url">
    <a :href="withBase(item.url)">{{ item.title }}</a><span v-if="item.summary">: {{ item.summary }}</span>
  </li>
</ul>

## Missing from shipped features

<p v-if="!data.gaps.length">None.</p>
<ul v-else>
  <li v-for="(gap, i) in data.gaps" :key="i">
    <a :href="withBase(gap.pageUrl)">{{ gap.pageTitle }}</a>: {{ gap.text }}
  </li>
</ul>
