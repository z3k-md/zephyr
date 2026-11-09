import { describe, expect, it } from 'vitest';
import {
  QUERY_MAX_LINES,
  clampQueryLines,
  flattenQueryNewlines,
  queryBoxHeight,
  queryDocumentCaret,
  queryLineCount,
} from './queryField';

const line = 25.2;
const pad = 8;
const border = 0;

describe('clampQueryLines', () => {
  it('keeps a count inside 1..4', () => {
    expect(clampQueryLines(0)).toBe(1);
    expect(clampQueryLines(1)).toBe(1);
    expect(clampQueryLines(1.4)).toBe(1);
    expect(clampQueryLines(1.5)).toBe(2);
    expect(clampQueryLines(4)).toBe(4);
    expect(clampQueryLines(4.6)).toBe(QUERY_MAX_LINES);
    expect(clampQueryLines(9)).toBe(4);
    expect(clampQueryLines(Number.NaN)).toBe(1);
  });
});

describe('queryLineCount', () => {
  it('reads a border box as whole lines and clamps at the cap', () => {
    expect(queryLineCount(line + pad, line, pad, border)).toBe(1);
    expect(queryLineCount(line * 2 + pad, line, pad, border)).toBe(2);
    expect(queryLineCount(line * 4 + pad, line, pad, border)).toBe(4);
    expect(queryLineCount(line * 6 + pad, line, pad, border)).toBe(4);
    // A pixel of measurement noise stays on the line it came from.
    expect(queryLineCount(line + pad + 1, line, pad, border)).toBe(1);
    expect(queryLineCount(0, line, pad, border)).toBe(1);
    expect(queryLineCount(40, 0, pad, border)).toBe(1);
  });

  it('counts past the cap when asked, so the field knows to scroll', () => {
    expect(queryLineCount(line * 6 + pad, line, pad, border, 100)).toBe(6);
  });
});

describe('queryBoxHeight', () => {
  it('is one line box, and a fifth line is the same height as the fourth', () => {
    expect(queryBoxHeight(line, pad, border, 1)).toBe(line + pad);
    expect(queryBoxHeight(line, pad, border, 3)).toBe(line * 3 + pad);
    expect(queryBoxHeight(line, pad, border, 5)).toBe(queryBoxHeight(line, pad, border, 4));
    expect(queryBoxHeight(line, pad, 2, 2)).toBe(line * 2 + pad + 2);
  });
});

describe('flattenQueryNewlines', () => {
  it('turns each line break into a space and leaves the rest of the text', () => {
    expect(flattenQueryNewlines('what is apixaban')).toBe('what is apixaban');
    expect(flattenQueryNewlines('a\nb')).toBe('a b');
    expect(flattenQueryNewlines('a\r\nb')).toBe('a b');
    expect(flattenQueryNewlines('a\rb')).toBe('a b');
    expect(flattenQueryNewlines('a\n\nb')).toBe('a  b');
    expect(flattenQueryNewlines('a\nb\n')).toBe('a b ');
    expect(flattenQueryNewlines('a\u2028b\u2029c')).toBe('a b c');
    expect(flattenQueryNewlines('')).toBe('');
  });
});

describe('queryDocumentCaret', () => {
  it('moves a collapsed caret to either end of the whole value', () => {
    expect(queryDocumentCaret('end', 12, 4, 4, 'none', false)).toEqual({
      start: 12,
      end: 12,
      direction: 'none',
    });
    expect(queryDocumentCaret('start', 12, 4, 4, 'none', false)).toEqual({
      start: 0,
      end: 0,
      direction: 'none',
    });
  });

  it('extends toward that end and keeps the anchor', () => {
    expect(queryDocumentCaret('end', 12, 3, 3, 'none', true)).toEqual({
      start: 3,
      end: 12,
      direction: 'forward',
    });
    expect(queryDocumentCaret('start', 12, 3, 8, 'forward', true)).toEqual({
      start: 0,
      end: 3,
      direction: 'backward',
    });
    expect(queryDocumentCaret('end', 12, 2, 6, 'backward', true)).toEqual({
      start: 6,
      end: 12,
      direction: 'forward',
    });
  });
});
