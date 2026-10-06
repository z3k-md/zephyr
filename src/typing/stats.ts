// Typing test scoring, following Monkeytype's definitions.

/** Words per minute, where a word is five characters. */
export function wpm(chars: number, ms: number): number {
  if (ms <= 0) return 0;
  return chars / 5 / (ms / 60000);
}

export interface Tally {
  /** Characters of correctly typed words, plus the spaces after them. */
  correctWordChars: number;
  /** Every character typed, spaces included. */
  allChars: number;
  correct: number;
  incorrect: number;
  extra: number;
  missed: number;
}

/**
 * Scores what was typed. `finished` words are the ones the user moved past; the last word
 * counts only as far as it was typed, and with no trailing space.
 */
export function tally(words: string[], typed: string[], finished: number): Tally {
  const result: Tally = {
    correctWordChars: 0,
    allChars: 0,
    correct: 0,
    incorrect: 0,
    extra: 0,
    missed: 0,
  };
  const count = Math.min(typed.length, words.length);
  for (let index = 0; index < count; index++) {
    const target = words[index];
    const input = typed[index] ?? '';
    const done = index < finished;
    const space = done && index < words.length - 1 ? 1 : 0;
    result.allChars += input.length + space;
    for (let at = 0; at < input.length; at++) {
      if (at >= target.length) result.extra++;
      else if (input[at] === target[at]) result.correct++;
      else result.incorrect++;
    }
    if (done && input.length < target.length) result.missed += target.length - input.length;
    const right = done ? input === target : target.startsWith(input) && input.length > 0;
    if (right) result.correctWordChars += input.length + space;
  }
  return result;
}

/** Monkeytype's consistency: 100 when every second's raw speed matches, lower as they vary. */
export function consistency(raws: number[]): number {
  const values = raws.filter((value) => Number.isFinite(value));
  if (values.length < 2) return 100;
  const mean = values.reduce((sum, value) => sum + value, 0) / values.length;
  if (mean === 0) return 0;
  const variance = values.reduce((sum, value) => sum + (value - mean) ** 2, 0) / values.length;
  const cov = Math.sqrt(variance) / mean;
  const kogasa = 100 * (1 - Math.tanh(cov + cov ** 3 / 3 + cov ** 5 / 5));
  return Math.max(0, Math.min(100, kogasa));
}
