/** How many history rows stay on screen at once. */
export const VISIBLE_ROWS = 5;

/**
 * Half-open slice of a newest-first list that keeps `at` visible.
 * A list shorter than the window is returned whole. Otherwise the selection sits
 * near the middle, pinned to either end so the slice never runs past the list.
 */
export function visibleRange(
  length: number,
  at: number,
  visible = VISIBLE_ROWS
): { start: number; end: number } {
  if (length <= 0) return { start: 0, end: 0 };
  const raw = Math.trunc(at);
  const index = Number.isFinite(raw) ? Math.min(length - 1, Math.max(0, raw)) : 0;
  if (length <= visible) return { start: 0, end: length };
  const start = Math.max(0, Math.min(index - Math.floor(visible / 2), length - visible));
  return { start, end: start + visible };
}
