import { createContentLoader } from 'vitepress';

export interface RoadmapEntry {
  title: string;
  url: string;
  summary: string;
}

export interface RoadmapInProgress extends RoadmapEntry {
  gaps: string[];
}

export interface RoadmapGap {
  pageTitle: string;
  pageUrl: string;
  text: string;
}

export interface RoadmapData {
  inProgress: RoadmapInProgress[];
  planned: RoadmapEntry[];
  gaps: RoadmapGap[];
}

function byTitle<T extends { title: string }>(a: T, b: T) {
  return a.title.localeCompare(b.title, undefined, { sensitivity: 'base' });
}

export default createContentLoader('features/*.md', {
  transform(raw): RoadmapData {
    const inProgress: RoadmapInProgress[] = [];
    const planned: RoadmapEntry[] = [];
    const gaps: RoadmapGap[] = [];

    for (const page of raw) {
      const fm = page.frontmatter;
      const title = typeof fm.title === 'string' ? fm.title : page.url;
      const summary = typeof fm.summary === 'string' ? fm.summary : '';
      const status = fm.status;
      const entry: RoadmapEntry = { title, url: page.url, summary };

      if (status === 'in-progress') {
        const pageGaps = Array.isArray(fm.gaps) ? fm.gaps.map(String) : [];
        inProgress.push({ ...entry, gaps: pageGaps });
      } else if (status === 'planned') {
        planned.push(entry);
      }

      if (status === 'shipped' || status === 'partial') {
        const pageGaps = Array.isArray(fm.gaps) ? fm.gaps : [];
        for (const gap of pageGaps) {
          gaps.push({
            pageTitle: title,
            pageUrl: page.url,
            text: String(gap),
          });
        }
      }
    }

    inProgress.sort(byTitle);
    planned.sort(byTitle);
    gaps.sort((a, b) => a.text.localeCompare(b.text, undefined, { sensitivity: 'base' }));

    return { inProgress, planned, gaps };
  },
});
