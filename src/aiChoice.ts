// One AI in the bar: the built-in Ask AI or a web AI (ChatGPT, Claude…), picked in
// Settings › AI. Only that one shows in Search with and takes Tab and ⌘↵; the others are
// still reachable from ⌘K.

import type { Destination, Snapshot } from './types';

/** Built-in destinations that are AI chats on the web. */
const WEB_AIS = ['chatgpt', 'claude', 'perplexity', 'gemini', 'copilot', 'grok'];

export function isAiLike(destination: Destination): boolean {
  return destination.kind === 'ai' || WEB_AIS.includes(destination.id);
}

/** The chosen AI; with no choice saved yet, the first AI in the list. */
export function chosenAi(snapshot: Snapshot | null): Destination | undefined {
  const options = (snapshot?.destinations ?? []).filter(
    (destination) => !destination.disabled && isAiLike(destination)
  );
  return options.find((destination) => destination.id === snapshot?.aiChoice) ?? options[0];
}

/** Whether a destination is an AI other than the chosen one, which the bar leaves out. */
export function isOtherAi(snapshot: Snapshot | null, destination: Destination): boolean {
  return isAiLike(destination) && destination.id !== chosenAi(snapshot)?.id;
}

/** "gpt-6-luna" → "GPT-6 Luna", "claude-sonnet-5" → "Claude Sonnet 5". */
function prettyModel(model: string): string {
  return model
    .split(/[-_\s]+/)
    .filter(Boolean)
    .map((part) => {
      const lower = part.toLowerCase();
      if (['gpt', 'ai', 'llm'].includes(lower)) return lower.toUpperCase();
      if (/^\d/.test(part)) return part;
      return lower.charAt(0).toUpperCase() + lower.slice(1);
    })
    .join(' ')
    .replace(/^GPT (\d)/, 'GPT-$1');
}

/** What the built-in Ask AI is called in the bar: its model, or else "Ask AI". */
export function askAiName(snapshot: Snapshot | null): string {
  const model = snapshot?.ai.model.trim();
  return model ? prettyModel(model) : 'Ask AI';
}

/** A destination's name in the bar, with Ask AI named after its model. */
export function displayName(snapshot: Snapshot | null, destination: Destination): string {
  return destination.kind === 'ai' ? askAiName(snapshot) : destination.name;
}
