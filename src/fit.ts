// The bar's windows never scroll a list: a list shows only the rows that fit, and the
// best-ranked come first, so typing more is how to reach the rest. `v-fit` hides the rows
// that would overflow the list's box (and a section header left with no rows) and reports
// how many are shown, so arrow keys can stop at the last one.

import type { Directive } from 'vue';

type OnCount = ((count: number) => void) | undefined;

function fit(list: HTMLElement, onCount: OnCount) {
  const options = list.querySelectorAll<HTMLElement>('[role="option"]');
  const rows = Array.from(
    options.length ? options : list.querySelectorAll<HTMLElement>(':scope > li')
  );
  for (const row of rows) row.style.display = '';
  const groups = Array.from(list.children) as HTMLElement[];
  for (const group of groups) group.style.display = '';
  const style = getComputedStyle(list);
  const bottom = list.getBoundingClientRect().bottom - parseFloat(style.paddingBottom || '0');
  let shown = 0;
  let full = false;
  for (const row of rows) {
    if (full || row.getBoundingClientRect().bottom > bottom + 0.5) {
      full = true;
      row.style.display = 'none';
    } else {
      shown += 1;
    }
  }
  // A section header with none of its rows left goes too.
  for (const group of groups) {
    const items = group.querySelectorAll<HTMLElement>('[role="option"]');
    if (items.length && Array.from(items).every((row) => row.style.display === 'none')) {
      group.style.display = 'none';
    }
  }
  onCount?.(shown);
}

const observers = new WeakMap<HTMLElement, ResizeObserver>();

export const vFit: Directive<HTMLElement, OnCount> = {
  mounted(list, binding) {
    fit(list, binding.value);
    const observer = new ResizeObserver(() => fit(list, binding.value));
    observer.observe(list);
    observers.set(list, observer);
  },
  updated(list, binding) {
    fit(list, binding.value);
  },
  unmounted(list) {
    observers.get(list)?.disconnect();
    observers.delete(list);
  },
};
