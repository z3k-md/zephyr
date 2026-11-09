import { describe, expect, it } from 'vitest';
import { VISIBLE_ROWS, visibleRange } from './window';

describe('visibleRange', () => {
  it('pins the window to the start', () => {
    expect(visibleRange(10, 0)).toEqual({ start: 0, end: VISIBLE_ROWS });
    expect(visibleRange(10, 1)).toEqual({ start: 0, end: VISIBLE_ROWS });
    expect(visibleRange(10, 2)).toEqual({ start: 0, end: VISIBLE_ROWS });
  });

  it('keeps the selection in the middle', () => {
    expect(visibleRange(10, 5)).toEqual({ start: 3, end: 8 });
    expect(visibleRange(12, 6)).toEqual({ start: 4, end: 9 });
  });

  it('pins the window to the end', () => {
    expect(visibleRange(10, 7)).toEqual({ start: 5, end: 10 });
    expect(visibleRange(10, 8)).toEqual({ start: 5, end: 10 });
    expect(visibleRange(10, 9)).toEqual({ start: 5, end: 10 });
  });

  it('shows a list shorter than the window whole', () => {
    expect(visibleRange(0, 0)).toEqual({ start: 0, end: 0 });
    expect(visibleRange(1, 0)).toEqual({ start: 0, end: 1 });
    expect(visibleRange(3, 2)).toEqual({ start: 0, end: 3 });
    expect(visibleRange(VISIBLE_ROWS, 4)).toEqual({ start: 0, end: VISIBLE_ROWS });
  });
});
