// Spoken text into a bar query and what letting go of the hotkey should do.
// The model returns capitalized, punctuated sentences. Command words and scope words
// are whole words only, at the start or the end, never inside another word.

/** `type` only fills the box and waits for Enter; the rest run when the key is let go. */
export type VoiceAction = 'type' | 'top' | 'search' | 'ask' | 'cancel';

export interface VoiceCommand {
  query: string;
  action: VoiceAction;
}

/** Trailing words. `go` and `enter` are only recognized here, not as a leading verb. */
const TRAILING: Record<string, VoiceAction> = {
  search: 'search',
  google: 'search',
  ask: 'ask',
  open: 'top',
  go: 'top',
  enter: 'top',
  cancel: 'cancel',
  nevermind: 'cancel',
};

/** Leading verbs. A lone word ("search", "open") is left as plain text. */
const LEADING: Record<string, VoiceAction> = {
  search: 'search',
  google: 'search',
  ask: 'ask',
  open: 'top',
};

/** Spoken scope words, only when more words follow. Values are the bar's own prefixes. */
const SCOPES: Record<string, string> = {
  file: '!f ',
  files: '!f ',
  bookmark: '!bm ',
  bookmarks: '!bm ',
  setting: '!set ',
  settings: '!set ',
  app: '!app ',
  apps: '!app ',
  clipboard: '!clip ',
  clip: '!clip ',
  note: '!note ',
  emoji: '!emoji ',
  shell: '> ',
  terminal: '> ',
};

const EDGE = /^[.,!?\s]+|[.,!?\s]+$/g;

/** The model writes "11 p.m."; the power commands read "11 pm". */
const MERIDIEM = /\b([ap])\.\s?m\b\.?/gi;

/** A breath before the words often comes out as one of these. */
const FILLER = /^(?:(?:um+|uh+|uhm|erm|ah+|hmm+|mm+|mhm)[.,!?]*\s+)+/i;
const FILLER_ONLY = /^(?:um+|uh+|uhm|erm|ah+|hmm+|mm+|mhm|okay|ok)$/i;

function normalize(text: string): string {
  const cleaned = text
    .replace(MERIDIEM, (_, letter: string) => `${letter.toLowerCase()}m`)
    .replace(EDGE, '')
    .replace(/\s+/g, ' ')
    .replace(FILLER, '')
    .trim();
  return FILLER_ONLY.test(cleaned) ? '' : cleaned;
}

/** The word a token is, ignoring punctuation stuck to it ("open,"). */
function tokenKey(token: string): string {
  return token.replace(/^[.,!?]+|[.,!?]+$/g, '').toLowerCase();
}

function withScope(query: string, action: VoiceAction): VoiceCommand {
  const cleaned = normalize(query);
  if (!cleaned) return { query: '', action };
  const parts = cleaned.split(' ');
  const prefix = parts.length > 1 ? SCOPES[tokenKey(parts[0])] : undefined;
  if (!prefix) return { query: cleaned, action };
  return { query: prefix + parts.slice(1).join(' '), action };
}

export function parseVoice(text: string): VoiceCommand {
  const normalized = normalize(text);
  if (!normalized) return { query: '', action: 'type' };
  const lower = normalized.toLowerCase();
  if (lower === 'cancel' || lower === 'never mind' || lower === 'nevermind') {
    return { query: '', action: 'cancel' };
  }
  const phrase = ' never mind';
  if (lower.endsWith(phrase)) {
    return withScope(normalized.slice(0, -phrase.length), 'cancel');
  }
  const words = normalized.split(' ');
  const last = tokenKey(words[words.length - 1] ?? '');
  if (words.length > 1 && TRAILING[last]) {
    return withScope(words.slice(0, -1).join(' '), TRAILING[last]);
  }
  const first = tokenKey(words[0] ?? '');
  if (words.length > 1 && LEADING[first]) {
    return withScope(words.slice(1).join(' '), LEADING[first]);
  }
  return withScope(normalized, 'type');
}
