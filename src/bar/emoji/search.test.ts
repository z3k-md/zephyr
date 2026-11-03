import { describe, expect, it } from 'vitest';
import { emojiSections, matchTier, rankEmoji, type EmojiItem } from './search';

function item(
  char: string,
  name: string,
  keywords: string[],
  kind: 'emoji' | 'symbol',
  group = kind === 'emoji' ? 'Smileys & Emotion' : 'Arrows'
): EmojiItem {
  return { char, name, keywords, group, skin: false, kind };
}

const fire = item('🔥', 'fire', ['flame'], 'emoji');
const engine = item('🚒', 'fire engine', ['truck'], 'emoji', 'Travel & Places');
const grin = item('😀', 'grinning face', ['face', 'grin'], 'emoji');
const arrow = item('→', 'rightwards arrow', ['arrow', 'right'], 'symbol');
const left = item('←', 'leftwards arrow', ['arrow', 'left'], 'symbol');
const items = [grin, fire, engine, arrow, left];

describe('rankEmoji', () => {
  it('orders exact name, then name prefix, then keyword prefix, then substring', () => {
    expect(rankEmoji(items, 'fire', []).map((entry) => entry.char)).toEqual(['🔥', '🚒']);
    expect(matchTier(fire, 'flame')).toBe(2);
    expect(matchTier(grin, 'ning')).toBe(3);
    expect(rankEmoji(items, 'arrow', []).map((entry) => entry.name)).toEqual([
      'leftwards arrow',
      'rightwards arrow',
    ]);
  });

  it('boosts a recent match ahead of the same tier only', () => {
    const cat = item('🐱', 'cat', [], 'emoji');
    const car = item('🚗', 'car', [], 'emoji', 'Travel & Places');
    expect(rankEmoji([car, cat], 'ca', ['🐱']).map((entry) => entry.name)).toEqual(['cat', 'car']);
    // An exact name still beats a recent prefix.
    expect(rankEmoji(items, 'fire', ['🚒']).map((entry) => entry.char)).toEqual(['🔥', '🚒']);
  });
});

describe('emojiSections', () => {
  it('shows recents, then emoji by category, then symbols', () => {
    const sections = emojiSections(items, '', ['🔥', '→']);
    expect(sections.map((section) => section.title)).toEqual([
      'Recent',
      'Smileys & Emotion',
      'Travel & Places',
      'Arrows',
    ]);
    expect(sections[0].items.map((entry) => entry.char)).toEqual(['🔥', '→']);
    const later = sections.slice(1).flatMap((section) => section.items.map((entry) => entry.char));
    expect(later).not.toContain('🔥');
    expect(later).not.toContain('→');
  });

  it('searches into Recent, Emoji, and Symbols', () => {
    const sections = emojiSections(items, 'arrow', ['→']);
    expect(sections.map((section) => section.title)).toEqual(['Recent', 'Symbols']);
    expect(sections[0].items.map((entry) => entry.char)).toEqual(['→']);
    expect(sections[1].items.map((entry) => entry.char)).toEqual(['←']);
  });
});
