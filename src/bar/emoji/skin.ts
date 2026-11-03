// Skin tones are the five Fitzpatrick modifiers. Tone 0 leaves the emoji as Unicode
// publishes it. A tone is inserted after each modifier base, and the variation selector
// (U+FE0F) is dropped, which is the fully-qualified form of a toned emoji.

const TONE_BASE = 0x1f3fb;
const VARIATION_SELECTOR = 0xfe0f;

/** 0 is the yellow default; 1–5 are the five tones. Anything else becomes 0. */
export function clampSkin(tone: number): number {
  if (!Number.isInteger(tone) || tone < 0 || tone > 5) return 0;
  return tone;
}

/** The emoji with `tone` applied. Tone 0, and a character that takes no tone, come back unchanged. */
export function withSkin(char: string, tone: number, bases: ReadonlySet<number>): string {
  const skin = clampSkin(tone);
  if (skin === 0) return char;
  const points: number[] = [];
  for (const piece of char) {
    const point = piece.codePointAt(0);
    if (point === undefined || point === VARIATION_SELECTOR) continue;
    points.push(point);
  }
  if (points.length === 0) return char;
  const modifier = String.fromCodePoint(TONE_BASE + skin - 1);
  let applied = false;
  let out = '';
  for (const point of points) {
    out += String.fromCodePoint(point);
    if (bases.has(point)) {
      out += modifier;
      applied = true;
    }
  }
  if (applied) return out;
  return (
    String.fromCodePoint(points[0]) +
    modifier +
    points
      .slice(1)
      .map((point) => String.fromCodePoint(point))
      .join('')
  );
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
