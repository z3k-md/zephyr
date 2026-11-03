import { describe, expect, it } from 'vitest';
import { clampSkin, codePointLabel, numericEntity, withSkin } from './skin';

const thumb = 0x1f44d;
const man = 0x1f468;
const woman = 0x1f469;

describe('withSkin', () => {
  it('inserts a Fitzpatrick modifier after each base and drops the variation selector', () => {
    const bases = new Set([thumb, man, woman]);
    expect(withSkin('\u{1F44D}', 0, bases)).toBe('\u{1F44D}');
    expect(withSkin('\u{1F44D}', 1, bases)).toBe('\u{1F44D}\u{1F3FB}');
    expect(withSkin('\u263A\uFE0F', 2, new Set([0x263a]))).toBe('\u263A\u{1F3FC}');
    expect(withSkin('\u{1F468}\u200D\u{1F469}', 1, bases)).toBe(
      '\u{1F468}\u{1F3FB}\u200D\u{1F469}\u{1F3FB}'
    );
  });

  it('tones the first character when the base set is empty', () => {
    expect(withSkin('\u{1F44D}', 3, new Set())).toBe('\u{1F44D}\u{1F3FD}');
    expect(clampSkin(9)).toBe(0);
    expect(clampSkin(4)).toBe(4);
  });
});

describe('code points and entities', () => {
  it('lists each code point', () => {
    expect(codePointLabel('🔥')).toBe('U+1F525');
    expect(numericEntity('©')).toBe('&#xA9;');
    expect(codePointLabel('\u{1F44D}\u{1F3FB}')).toBe('U+1F44D U+1F3FB');
  });
});
