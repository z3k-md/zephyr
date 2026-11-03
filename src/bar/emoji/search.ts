// Ranking for the emoji grid. An exact name wins, then a name that starts with the whole
// phrase, then a keyword prefix, then a substring of either. Several words also match
// when every word is a prefix of a different word in the name or the keywords, and that
// ranks last. A recent match sorts ahead of the same kind of match that isn't recent,
// and no further.

export interface EmojiItem {
  char: string;
  name: string;
  keywords: string[];
  group: string;
  skin: boolean;
  /** Code-point indexes a skin tone is inserted after. Missing when the emoji isn't toned. */
  tones?: readonly number[];
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

/** Words of the name and of each keyword. A keyword like "holding hands" is two words. */
function wordsOf(item: EmojiItem): string[] {
  const words = item.name.toLowerCase().split(' ');
  for (const keyword of item.keywords) words.push(...keyword.toLowerCase().split(' '));
  return words.filter(Boolean);
}

/** Every token is a prefix of a different word. One word is already covered by the tiers above. */
function tokensMatch(item: EmojiItem, wanted: string): boolean {
  const tokens = wanted.split(' ');
  if (tokens.length < 2) return false;
  const words = wordsOf(item);
  const used = new Set<number>();
  for (const token of tokens) {
    const index = words.findIndex((word, i) => !used.has(i) && word.startsWith(token));
    if (index < 0) return false;
    used.add(index);
  }
  return true;
}

/** 0 exact name, 1 whole-phrase prefix, 2 keyword prefix, 3 substring, 4 every word. */
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
  if (tokensMatch(item, wanted)) return 4;
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
 * With a query: Emoji, then Symbols, in match-tier order. Recency only breaks ties
 * inside a tier, so a recent weaker match doesn't jump ahead of a better one.
 */
export function emojiSections(
  items: readonly EmojiItem[],
  query: string,
  recents: readonly string[]
): EmojiSection[] {
  const wanted = normalize(query);
  if (!wanted) {
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
    const rest = items.filter((item) => !seen.has(item.char));
    const sections: EmojiSection[] = [];
    if (recentItems.length) sections.push({ title: 'Recent', kind: 'recent', items: recentItems });
    sections.push(...grouped(rest, 'emoji'), ...grouped(rest, 'symbol'));
    return sections;
  }

  const ranked = rankEmoji(items, wanted, recents);
  const emojiMatches = ranked.filter((item) => item.kind === 'emoji');
  const symbolMatches = ranked.filter((item) => item.kind === 'symbol');
  const sections: EmojiSection[] = [];
  if (emojiMatches.length) sections.push({ title: 'Emoji', kind: 'emoji', items: emojiMatches });
  if (symbolMatches.length)
    sections.push({ title: 'Symbols', kind: 'symbol', items: symbolMatches });
  return sections;
}
