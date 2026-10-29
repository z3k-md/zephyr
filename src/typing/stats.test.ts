import { describe, expect, it } from 'vitest';
import { consistency, tally, wpm } from './stats';

describe('wpm', () => {
  it.each([
    [50, 60000, 10],
    [250, 30000, 100],
    [10, 0, 0],
    [10, -5, 0],
  ])('%i chars in %i ms is %i', (chars, ms, expected) => {
    expect(wpm(chars, ms)).toBeCloseTo(expected);
  });
});

describe('tally', () => {
  const none = { correctWordChars: 0, allChars: 0, correct: 0, incorrect: 0, extra: 0, missed: 0 };
  it.each([
    {
      name: 'all right, the last word unfinished',
      words: ['the', 'cat', 'sat'],
      typed: ['the', 'cat', 'sa'],
      finished: 2,
      expected: { ...none, correctWordChars: 10, allChars: 10, correct: 8 },
    },
    {
      name: 'a wrong letter',
      words: ['the', 'cat'],
      typed: ['thx', 'cat'],
      finished: 1,
      expected: { ...none, correctWordChars: 3, allChars: 7, correct: 5, incorrect: 1 },
    },
    {
      name: 'extra and missed letters',
      words: ['the', 'cat', 'sat'],
      typed: ['thee', 'ca', ''],
      finished: 2,
      expected: { ...none, allChars: 8, correct: 5, extra: 1, missed: 1 },
    },
    {
      name: 'no space after the last word',
      words: ['go'],
      typed: ['go'],
      finished: 1,
      expected: { ...none, correctWordChars: 2, allChars: 2, correct: 2 },
    },
  ])('$name', ({ words, typed, finished, expected }) => {
    expect(tally(words, typed, finished)).toEqual(expected);
  });
});

describe('consistency', () => {
  it.each([
    [[], 100],
    [[60], 100],
    [[60, 60, 60], 100],
    [[0, 0], 0],
  ])('%j is %i', (raws, expected) => {
    expect(consistency(raws)).toBeCloseTo(expected);
  });

  it('falls as speeds vary', () => {
    const steady = consistency([60, 62, 58, 61]);
    const shaky = consistency([20, 100, 40, 90]);
    expect(steady).toBeGreaterThan(shaky);
    expect(shaky).toBeGreaterThanOrEqual(0);
  });

  it('ignores readings that are not numbers', () => {
    expect(consistency([60, NaN, 60, Infinity])).toBeCloseTo(100);
  });
});
