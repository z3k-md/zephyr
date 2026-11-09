// The search field is one paragraph: it wraps, but a newline never stays in the text.
// Line count and caret math live here so they can be tested without a textarea.

/** How many lines the field grows before the text scrolls inside it. */
export const QUERY_MAX_LINES = 4;

/** `lines` as an integer from 1 through `maxLines`. */
export function clampQueryLines(lines: number, maxLines = QUERY_MAX_LINES): number {
  const cap = Number.isFinite(maxLines)
    ? Math.max(1, Math.floor(maxLines))
    : Number.POSITIVE_INFINITY;
  if (!Number.isFinite(lines)) return 1;
  return Math.min(cap, Math.max(1, Math.round(lines)));
}

/**
 * How many wrapped lines a border-box measurement is. A fraction of a line rounds to the
 * nearest one, and the result stays inside 1..maxLines.
 */
export function queryLineCount(
  borderBox: number,
  lineHeight: number,
  padding: number,
  border: number,
  maxLines = QUERY_MAX_LINES
): number {
  if (!(lineHeight > 0) || !Number.isFinite(borderBox)) return 1;
  const content = borderBox - padding - border;
  if (!(content > 0)) return 1;
  return clampQueryLines(content / lineHeight, maxLines);
}

/** Border-box height of `lines` of text. `lines` is clamped to 1..maxLines. */
export function queryBoxHeight(
  lineHeight: number,
  padding: number,
  border: number,
  lines: number,
  maxLines = QUERY_MAX_LINES
): number {
  return clampQueryLines(lines, maxLines) * lineHeight + padding + border;
}

/** Pasted and dictated breaks become spaces, so the field stays one paragraph. */
export function flattenQueryNewlines(text: string): string {
  return text.replace(/\r\n|[\n\r\u2028\u2029]/g, ' ');
}

export interface QueryCaret {
  start: number;
  end: number;
  direction: 'forward' | 'backward' | 'none';
}

/**
 * Home, End, and the Mac line-edge chords move through the whole value. A wrapped
 * textarea would otherwise stop at the visual line. Shift keeps the anchor.
 */
export function queryDocumentCaret(
  edge: 'start' | 'end',
  length: number,
  selectionStart: number,
  selectionEnd: number,
  direction: 'forward' | 'backward' | 'none',
  shiftKey: boolean
): QueryCaret {
  const pos = edge === 'start' ? 0 : Math.max(0, length);
  if (!shiftKey) return { start: pos, end: pos, direction: 'none' };
  const anchor = direction === 'backward' ? selectionEnd : selectionStart;
  const start = Math.min(anchor, pos);
  const end = Math.max(anchor, pos);
  return { start, end, direction: pos < anchor ? 'backward' : 'forward' };
}
