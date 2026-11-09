import type { Theme } from 'vitepress';
import DefaultTheme from 'vitepress/theme';
import { h } from 'vue';
import Downloads from './Downloads.vue';
import FeatureMeta from './FeatureMeta.vue';
import NotYet from './NotYet.vue';
import './custom.css';

export default {
  extends: DefaultTheme,
  Layout: () =>
    h(DefaultTheme.Layout, null, {
      'home-hero-info-after': () => h(Downloads),
      'doc-before': () => h(FeatureMeta),
      'doc-after': () => h(NotYet),
    }),
} satisfies Theme;
