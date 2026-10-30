// Past calculations and conversions: each answer copied from the bar is kept (on this
// computer, in the bar's local storage), and the last few show under a new answer, so an
// earlier result is an arrow and Enter away.

import { ref } from 'vue';

export interface PastAnswer {
  /** What was typed: "12 * 7". */
  question: string;
  /** What it came to: "84". */
  answer: string;
}

const KEY = 'zephyr.pastAnswers';
const LIMIT = 20;

function load(): PastAnswer[] {
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? '[]');
    return Array.isArray(saved)
      ? saved.filter(
          (entry) => typeof entry?.question === 'string' && typeof entry?.answer === 'string'
        )
      : [];
  } catch {
    return [];
  }
}

/** Newest first. */
export const pastAnswers = ref<PastAnswer[]>(typeof localStorage === 'undefined' ? [] : load());

/** Keeps a copied answer; the same question again moves it to the front. */
export function rememberAnswer(question: string, answer: string) {
  const asked = question.trim();
  if (!asked || !answer) return;
  const kept = pastAnswers.value.filter((entry) => entry.question !== asked);
  pastAnswers.value = [{ question: asked, answer }, ...kept].slice(0, LIMIT);
  try {
    localStorage.setItem(KEY, JSON.stringify(pastAnswers.value));
  } catch {
    // Storage full or off: the history just lasts until the bar reloads.
  }
}
