import { describe, expect, it } from 'vitest';
import { SYMBOLS } from './symbols';

const REQUIRED = [
  '—',
  '–',
  '…',
  '“',
  '”',
  '‘',
  '’',
  '«',
  '»',
  '•',
  '·',
  '©',
  '®',
  '™',
  '°',
  '§',
  '¶',
  '†',
  '⌘',
  '⌥',
  '⇧',
  '⌃',
  '⏎',
  '⌫',
  '⇥',
  '⎋',
  '←',
  'α',
  '½',
  '✓',
  '✗',
  '€',
];

describe('SYMBOLS', () => {
  it('is about 150 named signs, each character once', () => {
    expect(SYMBOLS.length).toBeGreaterThanOrEqual(140);
    expect(SYMBOLS.length).toBeLessThanOrEqual(180);
    expect(new Set(SYMBOLS.map((symbol) => symbol.char)).size).toBe(SYMBOLS.length);
    for (const symbol of SYMBOLS) {
      expect(symbol.name.length).toBeGreaterThan(0);
      if (symbol.entity) expect(symbol.entity).toMatch(/^&[A-Za-z][A-Za-z0-9]*;$/);
    }
    const chars = new Set(SYMBOLS.map((symbol) => symbol.char));
    for (const char of REQUIRED) expect(chars.has(char)).toBe(true);
  });
});
