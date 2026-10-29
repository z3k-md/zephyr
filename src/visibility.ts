// Whether anyone can see this window, for timers that only matter while someone can.

import { ref } from 'vue';

/** The bar on screen. The bar window sets it as it shows and hides (it only moves off
 *  screen, so the page itself never reads as hidden); other windows leave it on. */
export const barShown = ref(true);

/** The page not hidden: a closed Settings or Notes window is only hidden, not gone. */
export const pageShown = ref(typeof document === 'undefined' || !document.hidden);

if (typeof document !== 'undefined') {
  document.addEventListener('visibilitychange', () => (pageShown.value = !document.hidden));
}
