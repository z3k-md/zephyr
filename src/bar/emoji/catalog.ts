// The emoji catalog is loaded the first time the grid opens, not with the bar. Symbols
// are the hand-written list and stay in the bundle; the Unicode table is the chunk.
// Country and subdivision flags stay only on a Mac: elsewhere the UI font has no
// ligature for them and they show up as letters.

import { isMac } from '../../keys';
import type { EmojiItem } from './search';
import { SYMBOLS } from './symbols';

interface EmojiData {
  groups: string[];
  subgroups: string[];
  emoji: { c: string; n: string; g: number; u: number; s: number; p?: number[]; k: string[] }[];
}

export interface Catalog {
  items: EmojiItem[];
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

/**
 * Country flags (two regional indicators) and subdivision flags (tag sequences)
 * stay when `mac` is set. Every other subgroup stays on either platform.
 */
export function showsSubgroup(subgroup: string, mac: boolean): boolean {
  if (subgroup !== 'country-flag' && subgroup !== 'subdivision-flag') return true;
  return mac;
}

/** Emoji from the generated table, then symbols. A character in both is kept as a symbol,
 *  so its named HTML entity isn't dropped. */
export function build(data: EmojiData): Catalog {
  const items: EmojiItem[] = [];
  const symbols = new Set(SYMBOLS.map((symbol) => symbol.char));
  for (const entry of data.emoji) {
    if (symbols.has(entry.c)) continue;
    const subgroup = data.subgroups[entry.u] ?? '';
    if (!showsSubgroup(subgroup, isMac)) continue;
    items.push({
      char: entry.c,
      name: entry.n,
      keywords: entry.k,
      group: data.groups[entry.g] ?? 'Emoji',
      skin: entry.s === 1,
      tones: entry.p ?? [],
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
  return { items };
}
