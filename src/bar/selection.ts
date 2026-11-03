// Which row stays selected when the list changes without a new search: a timer
// tick, a meeting becoming soon, the soon row leaving. Identity is `rowKey`.

/** Same keys in the same order: a tick that only rewrote a label. */
function sameKeys(previous: readonly string[], next: readonly string[]): boolean {
  if (previous.length !== next.length) return false;
  for (let i = 0; i < previous.length; i++) if (previous[i] !== next[i]) return false;
  return true;
}

/**
 * The index to select after the rows change.
 * The same keys keep the index, so a ticking label doesn't move the highlight.
 * Once the user has moved, their row stays selected, or nothing does if it left.
 * Otherwise the list's new preselect applies.
 */
export function reconcileSelection(
  previousKeys: readonly string[],
  previousIndex: number,
  userMoved: boolean,
  nextKeys: readonly string[],
  preselect: number | null
): { index: number; userMoved: boolean } {
  if (sameKeys(previousKeys, nextKeys)) {
    const index = previousIndex >= 0 && previousIndex < nextKeys.length ? previousIndex : -1;
    return { index, userMoved };
  }
  if (userMoved) {
    const key = previousIndex >= 0 ? previousKeys[previousIndex] : undefined;
    const index = key ? nextKeys.indexOf(key) : -1;
    return { index, userMoved: true };
  }
  const index =
    preselect !== null && preselect >= 0 && preselect < nextKeys.length ? preselect : -1;
  return { index, userMoved: false };
}
