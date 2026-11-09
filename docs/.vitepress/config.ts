import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import matter from 'gray-matter';
import { defineConfig } from 'vitepress';
import type { DefaultTheme } from 'vitepress';

const configDir = path.dirname(fileURLToPath(import.meta.url));
const docsRoot = path.join(configDir, '..');
const featuresDir = path.join(docsRoot, 'features');

type SidebarItem = DefaultTheme.SidebarItem;

const GROUP_LABELS: Record<string, string> = {
  bar: 'Bar and search',
  utilities: 'Utilities',
  account: 'Account',
};

function statusMarker(status: string | undefined): string {
  if (status === 'planned') return ' (planned)';
  if (status === 'in-progress') return ' (in progress)';
  if (status === 'partial') return ' (partial)';
  return '';
}

type Ranked = { title: string; item: SidebarItem };

function buildFeaturesSidebar(): SidebarItem[] {
  const buckets: Record<'bar' | 'utilities' | 'account' | 'planned', Ranked[]> = {
    bar: [],
    utilities: [],
    account: [],
    planned: [],
  };

  let files: string[] = [];
  try {
    if (fs.existsSync(featuresDir)) {
      files = fs.readdirSync(featuresDir).filter((name) => name.endsWith('.md'));
    }
  } catch {
    return [];
  }

  for (const file of files) {
    const slug = file.replace(/\.md$/, '');
    let title = slug;
    let status: string | undefined;
    let group: string | undefined;
    try {
      const raw = fs.readFileSync(path.join(featuresDir, file), 'utf8');
      const { data } = matter(raw);
      title = typeof data.title === 'string' ? data.title : slug;
      status = typeof data.status === 'string' ? data.status : undefined;
      group = typeof data.group === 'string' ? data.group : undefined;
    } catch {
      // keep defaults
    }

    const item: SidebarItem = {
      text: `${title}${statusMarker(status)}`,
      link: `/features/${slug}`,
    };

    if (status === 'planned') {
      buckets.planned.push({ title, item });
    } else if (group === 'bar' || group === 'utilities' || group === 'account') {
      buckets[group].push({ title, item });
    }
  }

  const sortByTitle = (a: Ranked, b: Ranked) =>
    a.title.localeCompare(b.title, undefined, { sensitivity: 'base' });

  const sidebar: SidebarItem[] = [];
  for (const key of ['bar', 'utilities', 'account'] as const) {
    const items = buckets[key].sort(sortByTitle).map((entry) => entry.item);
    if (items.length > 0) {
      sidebar.push({ text: GROUP_LABELS[key], items });
    }
  }
  const planned = buckets.planned.sort(sortByTitle).map((entry) => entry.item);
  if (planned.length > 0) {
    sidebar.push({ text: 'Planned', items: planned });
  }
  return sidebar;
}

const guideGroup: SidebarItem = {
  text: 'Guide',
  items: [
    { text: 'Install', link: '/guide/install' },
    { text: 'The bar', link: '/guide/the-bar' },
  ],
};

const roadmapLink: SidebarItem = { text: 'Roadmap', link: '/roadmap' };

const featureBlocks = buildFeaturesSidebar();

const guideSidebar: SidebarItem[] = [guideGroup, roadmapLink];
const featuresSidebar: SidebarItem[] = [guideGroup, ...featureBlocks, roadmapLink];

const contributingSidebar: SidebarItem[] = [
  {
    text: 'Contributing',
    items: [
      { text: 'Develop', link: '/contributing/develop' },
      { text: 'Branches', link: '/contributing/branches' },
      { text: 'Release', link: '/contributing/release' },
      { text: 'How the docs work', link: '/contributing/docs' },
    ],
  },
];

export default defineConfig({
  title: 'Zephyr',
  description: 'A launcher bar for Windows and macOS.',
  base: '/zephyr/',
  cleanUrls: true,
  lastUpdated: true,
  themeConfig: {
    search: {
      provider: 'local',
    },
    editLink: {
      pattern: 'https://github.com/z3k-md/zephyr/edit/develop/docs/:path',
    },
    socialLinks: [{ icon: 'github', link: 'https://github.com/z3k-md/zephyr' }],
    nav: [
      { text: 'Guide', link: '/guide/install', activeMatch: '/guide/' },
      { text: 'Features', link: '/features/scopes', activeMatch: '/features/' },
      { text: 'Roadmap', link: '/roadmap' },
      { text: 'Contributing', link: '/contributing/develop', activeMatch: '/contributing/' },
      {
        text: 'Download',
        link: 'https://github.com/z3k-md/zephyr/releases/latest',
      },
    ],
    sidebar: {
      '/guide/': guideSidebar,
      '/features/': featuresSidebar,
      '/roadmap': featuresSidebar,
      '/contributing/': contributingSidebar,
    },
  },
});
