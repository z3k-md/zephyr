// Skin tones are the five Fitzpatrick modifiers. Tone 0 leaves the emoji as Unicode
// publishes it. `positions` are the code-point indexes that take a modifier, copied
// from Unicode's own single-tone sequence. A variation selector right after one of
// those indexes is dropped; a variation selector on any other part stays, so a
// profession's symbol keeps its emoji presentation.

const TONE_BASE = 0x1f3fb;
const VARIATION_SELECTOR = 0xfe0f;

/** 0 is the yellow default; 1–5 are the five tones. Anything else becomes 0. */
export function clampSkin(tone: number): number {
  if (!Number.isInteger(tone) || tone < 0 || tone > 5) return 0;
  return tone;
}

/**
 * The emoji with `tone` applied at `positions`. Tone 0, and an emoji with no
 * positions, come back unchanged. Positions come from the catalog; this does not
 * guess which character can take a modifier.
 */
export function withSkin(char: string, tone: number, positions: readonly number[]): string {
  const skin = clampSkin(tone);
  if (skin === 0 || positions.length === 0) return char;
  const points: number[] = [];
  for (const piece of char) {
    const point = piece.codePointAt(0);
    if (point === undefined) continue;
    points.push(point);
  }
  const slots = new Set(positions);
  const modifier = TONE_BASE + skin - 1;
  let out = '';
  for (let i = 0; i < points.length; i++) {
    const point = points[i];
    if (point === VARIATION_SELECTOR && slots.has(i - 1)) continue;
    out += String.fromCodePoint(point);
    if (slots.has(i)) out += String.fromCodePoint(modifier);
  }
  return out;
}

/** `U+1F525` for each code point, in order. */
export function codePointLabel(char: string): string {
  const labels: string[] = [];
  for (const piece of char) {
    const point = piece.codePointAt(0);
    if (point === undefined) continue;
    labels.push(`U+${point.toString(16).toUpperCase().padStart(4, '0')}`);
  }
  return labels.join(' ');
}

/** A numeric HTML character reference for each code point. */
export function numericEntity(char: string): string {
  const parts: string[] = [];
  for (const piece of char) {
    const point = piece.codePointAt(0);
    if (point === undefined) continue;
    parts.push(`&#x${point.toString(16).toUpperCase()};`);
  }
  return parts.join('');
}
