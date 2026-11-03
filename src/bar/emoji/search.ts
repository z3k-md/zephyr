// Ranking for the emoji grid. An exact name wins, then a name prefix, then a keyword
// prefix, then a substring of either. A recent match sorts ahead of the same kind of
// match that isn't recent, and no further.

export interface EmojiItem {
  char: string;
  name: string;
  keywords: string[];
  group: string;
  skin: boolean;
  kind: 'emoji' | 'symbol';
  /** A named HTML entity, when the character has one. */
  entity?: string;
}

export interface EmojiSection {
  title: string;
  kind: 'recent' | 'emoji' | 'symbol';
  items: EmojiItem[];
}

export const RECENT_LIMIT = 24;

function normalize(text: string): string {
  return text.trim().toLowerCase().replace(/\s+/g, ' ');
}

/** 0 exact name, 1 name prefix, 2 keyword prefix, 3 substring. Null when nothing matches. */
export function matchTier(item: EmojiItem, query: string): number | null {
  const wanted = normalize(query);
  if (!wanted) return null;
  const name = item.name.toLowerCase();
  if (name === wanted) return 0;
  if (name.startsWith(wanted)) return 1;
  if (item.keywords.some((word) => word.toLowerCase().startsWith(wanted))) return 2;
  if (name.includes(wanted) || item.keywords.some((word) => word.toLowerCase().includes(wanted))) {
    return 3;
  }
  return null;
}

function byName(a: EmojiItem, b: EmojiItem): number {
  if (a.name < b.name) return -1;
  if (a.name > b.name) return 1;
  return 0;
}

/** Matches, best tier first. Recents of a tier come before the rest of that tier. */
export function rankEmoji(
  items: readonly EmojiItem[],
  query: string,
  recents: readonly string[]
): EmojiItem[] {
  const recentAt = new Map(recents.map((char, index) => [char, index]));
  const ranked: { item: EmojiItem; tier: number; recent: boolean }[] = [];
  for (const item of items) {
    const tier = matchTier(item, query);
    if (tier === null) continue;
    ranked.push({ item, tier, recent: recentAt.has(item.char) });
  }
  ranked.sort((a, b) => {
    if (a.tier !== b.tier) return a.tier - b.tier;
    if (a.recent !== b.recent) return a.recent ? -1 : 1;
    if (a.recent && b.recent) {
      const order = (recentAt.get(a.item.char) ?? 0) - (recentAt.get(b.item.char) ?? 0);
      if (order !== 0) return order;
    }
    return byName(a.item, b.item);
  });
  return ranked.map((entry) => entry.item);
}

function grouped(items: readonly EmojiItem[], kind: 'emoji' | 'symbol'): EmojiSection[] {
  const groups = new Map<string, EmojiItem[]>();
  for (const item of items) {
    if (item.kind !== kind) continue;
    const list = groups.get(item.group);
    if (list) list.push(item);
    else groups.set(item.group, [item]);
  }
  return [...groups].map(([title, groupItems]) => ({ title, kind, items: groupItems }));
}

/**
 * What the grid shows. With no query: recents (newest first, up to 24), then emoji by
 * category, then symbols by kind. A recent character is not repeated in its category.
 * With a query: Recent, Emoji, and Symbols, each in rank order, omitting an empty one.
 */
export function emojiSections(
  items: readonly EmojiItem[],
  query: string,
  recents: readonly string[]
): EmojiSection[] {
  const byChar = new Map(items.map((item) => [item.char, item]));
  const recentItems: EmojiItem[] = [];
  const seen = new Set<string>();
  for (const char of recents) {
    if (seen.has(char) || recentItems.length >= RECENT_LIMIT) continue;
    const item = byChar.get(char);
    if (!item) continue;
    seen.add(char);
    recentItems.push(item);
  }

  const wanted = normalize(query);
  if (!wanted) {
    const rest = items.filter((item) => !seen.has(item.char));
    const sections: EmojiSection[] = [];
    if (recentItems.length) sections.push({ title: 'Recent', kind: 'recent', items: recentItems });
    sections.push(...grouped(rest, 'emoji'), ...grouped(rest, 'symbol'));
    return sections;
  }

  const ranked = rankEmoji(items, wanted, recents);
  const recentMatches = ranked.filter((item) => seen.has(item.char));
  const emojiMatches = ranked.filter((item) => item.kind === 'emoji' && !seen.has(item.char));
  const symbolMatches = ranked.filter((item) => item.kind === 'symbol' && !seen.has(item.char));
  const sections: EmojiSection[] = [];
  if (recentMatches.length)
    sections.push({ title: 'Recent', kind: 'recent', items: recentMatches });
  if (emojiMatches.length) sections.push({ title: 'Emoji', kind: 'emoji', items: emojiMatches });
  if (symbolMatches.length)
    sections.push({ title: 'Symbols', kind: 'symbol', items: symbolMatches });
  return sections;
}
