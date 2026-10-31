import { describe as group, expect, it } from 'vitest';
import { Limiter, describe } from './log';

group('Limiter', () => {
  it('drops the same line again within a minute, numbers and all', () => {
    const limiter = new Limiter();
    expect(limiter.allow('boom 12', 1_000)).toBe(true);
    expect(limiter.allow('boom 13', 5_000)).toBe(false);
    expect(limiter.allow('other', 5_000)).toBe(true);
    expect(limiter.allow('boom 14', 61_000)).toBe(true);
  });

  it('writes at most 30 lines a minute and counts the rest', () => {
    const limiter = new Limiter();
    const letters = (n: number) =>
      String.fromCharCode(97 + (n % 26)) + String.fromCharCode(97 + Math.floor(n / 26));
    const allowed = Array.from({ length: 40 }, (_, n) => limiter.allow(letters(n), 1_000));
    expect(allowed.filter(Boolean)).toHaveLength(30);
    expect(limiter.takeDropped()).toBe(10);
    expect(limiter.takeDropped()).toBe(0);
    expect(limiter.allow('next minute', 61_000)).toBe(true);
  });
});

group('describe', () => {
  it('keeps an error stack and writes objects as JSON', () => {
    const error = new Error('bad');
    expect(describe(error)).toContain('bad');
    expect(describe({ a: 1 })).toBe('{"a":1}');
    expect(describe(undefined)).toBe('undefined');
    const loop: Record<string, unknown> = {};
    loop.self = loop;
    expect(describe(loop)).toBe('[object Object]');
  });
});
