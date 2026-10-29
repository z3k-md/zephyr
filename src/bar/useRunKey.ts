// Alt+1..9 (⌘1..9 on a Mac) runs the Nth row. Holding the key a moment, or tapping Alt
// alone on Windows, numbers the rows; any other key hides the numbers again.

import { onUnmounted, ref } from 'vue';
import { isMac } from '../keys';

export function useRunKey() {
  const RUN_KEY = isMac ? 'cmd' : 'alt';
  const showNumbers = ref(false);
  let numbersTimer = 0;
  let altAlone = false;

  function runKeyHeld(event: KeyboardEvent | MouseEvent) {
    return isMac
      ? event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey
      : event.altKey && !event.ctrlKey && !event.metaKey && !event.shiftKey;
  }

  /** The row index Alt+digit names, or null when the key isn't that. */
  function rowFor(event: KeyboardEvent): number | null {
    if (!runKeyHeld(event) || !/^Digit[1-9]$/.test(event.code)) return null;
    return Number(event.code.slice(5)) - 1;
  }

  function track(event: KeyboardEvent | MouseEvent) {
    const keyboard = event instanceof KeyboardEvent;
    if (keyboard && event.type === 'keydown') {
      altAlone = !isMac && event.key === 'Alt';
      if (event.key !== 'Alt' && event.key !== 'Meta') showNumbers.value = false;
    }
    if (keyboard && event.type === 'keyup' && event.key === 'Alt' && !isMac) {
      // A bare Alt would otherwise move the webview's focus to a (missing) menu bar.
      event.preventDefault();
      window.clearTimeout(numbersTimer);
      numbersTimer = 0;
      showNumbers.value = altAlone;
      altAlone = false;
      return;
    }
    if (runKeyHeld(event)) {
      if (!numbersTimer && !showNumbers.value) {
        numbersTimer = window.setTimeout(() => {
          numbersTimer = 0;
          showNumbers.value = true;
        }, 300);
      }
    } else if (!keyboard || event.type === 'keyup') {
      window.clearTimeout(numbersTimer);
      numbersTimer = 0;
      if (isMac || keyboard) showNumbers.value = false;
    }
  }

  /** Alt+Tab away: the Alt reached the bar, its release didn't. */
  function reset() {
    window.clearTimeout(numbersTimer);
    numbersTimer = 0;
    showNumbers.value = false;
    altAlone = false;
  }

  onUnmounted(() => window.clearTimeout(numbersTimer));

  return { RUN_KEY, showNumbers, rowFor, track, reset };
}
