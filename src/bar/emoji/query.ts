// How typed text becomes an emoji search. `:3000` stays a port lookup; a colon only
// opens emoji when the next character is a letter. `!emoji` is a scope wherever it
// sits, the same as `!f`.

const EMOJI_BANG = /^!emoji$/i;

/** The text after `!emoji`, or null when that scope isn't in the text. */
export function takeEmojiScope(text: string): { query: string } | null {
  const tokens = text.trim().split(/\s+/).filter(Boolean);
  if (!tokens.some((token) => EMOJI_BANG.test(token))) return null;
  return { query: tokens.filter((token) => !EMOJI_BANG.test(token)).join(' ') };
}

/**
 * The search after a leading colon, when it starts with a letter (`:fire`, `:arrow`).
 * Null for `:3000` and any other `:<digits>`, which ask which process has that port.
 */
export function colonEmojiQuery(text: string): string | null {
  const trimmed = text.trim();
  if (!trimmed.startsWith(':')) return null;
  const rest = trimmed.slice(1).trim();
  if (!/^[A-Za-z]/.test(rest)) return null;
  return rest;
}

/** True when the text is a colon emoji search rather than a port. */
export function isEmojiColon(text: string): boolean {
  return colonEmojiQuery(text) !== null;
}
