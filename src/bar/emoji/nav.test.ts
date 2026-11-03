import { describe, expect, it } from 'vitest';
import { move, place } from './nav';

// Two columns. Three items, then a heading, then one item:
// row 1: 0 1
// row 2: 2
// row 4: 3
const cells = place([3, 1], 2);

describe('move', () => {
  it('steps across a row and onto the next one', () => {
    expect(move(cells, 0, 'right', 1)).toBe(1);
    expect(move(cells, 1, 'right', 1)).toBe(2);
    expect(move(cells, 2, 'right', 1)).toBe(3);
    expect(move(cells, 3, 'right', 1)).toBe(3);
    expect(move(cells, 0, 'left', 1)).toBe(0);
    expect(move(cells, 2, 'left', 1)).toBe(1);
  });

  it('moves up and down by column, including a short row and a page', () => {
    expect(move(cells, 0, 'down', 1)).toBe(2);
    expect(move(cells, 1, 'down', 1)).toBe(2);
    expect(move(cells, 3, 'up', 1)).toBe(2);
    expect(move(cells, 0, 'up', 1)).toBe(0);
    expect(move(cells, 0, 'pagedown', 1)).toBe(2);
    expect(move(cells, 0, 'pagedown', 10)).toBe(3);
    expect(move(cells, 3, 'pageup', 10)).toBe(0);
  });
});
