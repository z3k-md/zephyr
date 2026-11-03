// The emoji catalog is loaded the first time the grid opens, not with the bar. Symbols
// are the hand-written list and stay in the bundle; the Unicode table is the chunk.

import type { EmojiItem } from './search';
import { SYMBOLS } from './symbols';

interface EmojiData {
  groups: string[];
  bases: number[];
  emoji: { c: string; n: string; g: number; s: number; k: string[] }[];
}

export interface Catalog {
  items: EmojiItem[];
  bases: Set<number>;
}

let pending: Promise<Catalog> | null = null;

/** The catalog. A failed load can be tried again; a successful one is kept. */
export function loadCatalog(): Promise<Catalog> {
  if (!pending) {
    pending = import('./data.json')
      .then((mod) => build(unwrap(mod)))
      .catch((error: unknown) => {
        pending = null;
        throw error;
      });
  }
  return pending;
}

function unwrap(mod: unknown): EmojiData {
  const value =
    mod && typeof mod === 'object' && 'default' in mod
      ? (mod as { default: unknown }).default
      : mod;
  if (!value || typeof value !== 'object' || !('emoji' in value)) {
    throw new Error("Emoji data couldn't be read");
  }
  return value as EmojiData;
}

/** Emoji from the generated table, then symbols. A character in both is kept as a symbol,
 *  so its named HTML entity isn't dropped. */
export function build(data: EmojiData): Catalog {
  const items: EmojiItem[] = [];
  const symbols = new Set(SYMBOLS.map((symbol) => symbol.char));
  for (const entry of data.emoji) {
    if (symbols.has(entry.c)) continue;
    items.push({
      char: entry.c,
      name: entry.n,
      keywords: entry.k,
      group: data.groups[entry.g] ?? 'Emoji',
      skin: entry.s === 1,
      kind: 'emoji',
    });
  }
  for (const symbol of SYMBOLS) {
    items.push({
      char: symbol.char,
      name: symbol.name,
      keywords: symbol.keywords,
      group: symbol.group,
      skin: false,
      kind: 'symbol',
      entity: symbol.entity,
    });
  }
  return { items, bases: new Set(data.bases) };
}
