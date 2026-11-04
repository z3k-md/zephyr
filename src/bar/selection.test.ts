import { describe, expect, it } from 'vitest';
import { reconcileSelection } from './selection';

const keys = ['soon', 'focus', 'app'];

describe('reconcileSelection', () => {
  it('keeps the index when only a label changed', () => {
    const again = reconcileSelection(keys, 1, false, [...keys], 0);
    expect(again).toEqual({ index: 1, userMoved: false });
    const moved = reconcileSelection(keys, 2, true, [...keys], 0);
    expect(moved).toEqual({ index: 2, userMoved: true });
  });

  it('follows the row the user moved to, or selects nothing once it is gone', () => {
    const followed = reconcileSelection(keys, 0, true, ['focus', 'soon', 'app'], null);
    expect(followed).toEqual({ index: 1, userMoved: true });
    const gone = reconcileSelection(keys, 0, true, ['focus', 'app'], 0);
    expect(gone).toEqual({ index: -1, userMoved: true });
  });

  it('applies the new preselect when the user has not moved', () => {
    const appeared = reconcileSelection(['focus', 'app'], 0, false, keys, 0);
    expect(appeared).toEqual({ index: 0, userMoved: false });
    const dropped = reconcileSelection(keys, 0, false, ['focus', 'app'], 0);
    expect(dropped).toEqual({ index: 0, userMoved: false });
    const none = reconcileSelection(keys, 0, false, ['focus', 'app'], null);
    expect(none).toEqual({ index: -1, userMoved: false });
  });
});
