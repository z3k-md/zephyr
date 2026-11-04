import { describe, expect, it } from 'vitest';
import { clampSkin, codePointLabel, numericEntity, withSkin } from './skin';

describe('withSkin', () => {
  it('tones a simple emoji after its only base', () => {
    expect(withSkin('\u{1F44D}', 0, [0])).toBe('\u{1F44D}');
    expect(withSkin('\u{1F44D}', 1, [0])).toBe('\u{1F44D}\u{1F3FB}');
    expect(withSkin('\u{1F44D}', 3, [])).toBe('\u{1F44D}');
    expect(clampSkin(9)).toBe(0);
    expect(clampSkin(4)).toBe(4);
  });

  it('drops the variation selector on a toned base and keeps one that is not toned', () => {
    expect(withSkin('\u270C\uFE0F', 1, [0])).toBe('\u270C\u{1F3FB}');
    expect(withSkin('\u270D\uFE0F', 2, [0])).toBe('\u270D\u{1F3FC}');
    expect(withSkin('\u{1F469}\u200D\u2695\uFE0F', 1, [0])).toBe(
      '\u{1F469}\u{1F3FB}\u200D\u2695\uFE0F'
    );
  });

  it('tones the people holding hands and not the handshake between them', () => {
    expect(withSkin('\u{1F9D1}\u200D\u{1F91D}\u200D\u{1F9D1}', 1, [0, 4])).toBe(
      '\u{1F9D1}\u{1F3FB}\u200D\u{1F91D}\u200D\u{1F9D1}\u{1F3FB}'
    );
  });

  it('tones the person in a profession and leaves the object', () => {
    expect(withSkin('\u{1F9D1}\u200D\u{1F4BB}', 4, [0])).toBe('\u{1F9D1}\u{1F3FE}\u200D\u{1F4BB}');
  });
});

describe('code points and entities', () => {
  it('lists each code point', () => {
    expect(codePointLabel('🔥')).toBe('U+1F525');
    expect(numericEntity('©')).toBe('&#xA9;');
    expect(codePointLabel('\u{1F44D}\u{1F3FB}')).toBe('U+1F44D U+1F3FB');
  });
});
