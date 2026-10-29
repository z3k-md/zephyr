import { describe, expect, it } from 'vitest';
import { ago, bytes, elapsed } from './format';

describe('bytes', () => {
  it.each([
    [0, '0 B'],
    [512, '512 B'],
    [1024, '1.0 KB'],
    [1536, '1.5 KB'],
    [10 * 1024, '10 KB'],
    [4.2 * 1024 ** 3, '4.2 GB'],
    [2 * 1024 ** 5, '2048 TB'],
  ])('%d is %s', (value, text) => {
    expect(bytes(value)).toBe(text);
  });

  it('takes more digits when asked', () => {
    expect(bytes(1536, 2)).toBe('1.50 KB');
  });
});

describe('elapsed', () => {
  it.each([
    [-5, '0 s'],
    [40, '40 s'],
    [89.4, '89 s'],
    [89.6, '2 min'],
    [12 * 60, '12 min'],
    [89 * 60, '89 min'],
    [90 * 60, '2 h'],
    [3 * 3600, '3 h'],
  ])('%d s is %s', (seconds, text) => {
    expect(elapsed(seconds)).toBe(text);
  });
});

describe('ago', () => {
  const nowMs = Date.UTC(2026, 9, 29, 12);
  const before = (seconds: number) => nowMs / 1000 - seconds;

  it.each([
    [40, 'words', '40 s ago'],
    [12 * 60, 'words', '12 min ago'],
    [30, 'short', 'now'],
    [5 * 60, 'short', '5m'],
    [3 * 3600, 'short', '3h'],
    [4 * 86400, 'short', '4d'],
    [30, 'recent', 'just now'],
    [5 * 60, 'recent', '5m ago'],
    [3 * 3600, 'recent', '3h ago'],
    [-30, 'short', 'now'],
  ] as const)('%d s before, %s, is %s', (seconds, style, text) => {
    expect(ago(before(seconds), style, nowMs)).toBe(text);
  });

  it('gives the date once a day has passed, in the recent style', () => {
    const at = before(2 * 86400);
    expect(ago(at, 'recent', nowMs)).toBe(new Date(at * 1000).toLocaleDateString());
  });
});
