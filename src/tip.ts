// Tooltips drawn by the app instead of the system's: themed like the bar, and fed live, so a
// value that changes while one is open (a footer meter) updates in place rather than the
// system tooltip closing and reopening on every change. `v-tip="text"`; a newline starts a
// second, quieter line. One tooltip element serves the whole window.

import type { Directive } from 'vue';

/** How long the pointer rests before a tooltip shows; moving between tipped things right
 *  after one closed shows the next at once. */
const DELAY_MS = 450;
const WARM_MS = 400;
/** Space kept between the tooltip and the window's edges, and the thing it's for. */
const MARGIN = 6;

let tip: HTMLDivElement | null = null;
let anchor: HTMLElement | null = null;
let timer = 0;
let closedAt = 0;
const texts = new WeakMap<HTMLElement, string>();

function element(): HTMLDivElement {
  if (!tip) {
    tip = document.createElement('div');
    tip.className = 'app-tip';
    tip.setAttribute('role', 'tooltip');
    document.body.appendChild(tip);
  }
  return tip;
}

function render(text: string) {
  const box = element();
  const [first, ...rest] = text.split('\n');
  box.replaceChildren(document.createTextNode(first));
  for (const line of rest) {
    const extra = document.createElement('span');
    extra.className = 'app-tip-line';
    extra.textContent = line;
    box.appendChild(extra);
  }
}

/** Over the thing when there's room (footers sit at the bottom), else under it; kept inside
 *  the window, which is the bar's own size. */
function place(target: HTMLElement) {
  const box = element();
  const at = target.getBoundingClientRect();
  const size = box.getBoundingClientRect();
  const left = Math.min(
    Math.max(MARGIN, at.left + at.width / 2 - size.width / 2),
    window.innerWidth - size.width - MARGIN
  );
  const above = at.top - size.height - MARGIN;
  const top =
    above >= MARGIN
      ? above
      : Math.min(at.bottom + MARGIN, window.innerHeight - size.height - MARGIN);
  box.style.transform = `translate(${Math.round(left)}px, ${Math.round(top)}px)`;
}

function show(target: HTMLElement) {
  const text = texts.get(target);
  if (!text) return;
  anchor = target;
  render(text);
  const box = element();
  box.classList.add('shown');
  place(target);
}

function hide() {
  window.clearTimeout(timer);
  timer = 0;
  if (anchor) closedAt = performance.now();
  anchor = null;
  tip?.classList.remove('shown');
}

function enter(event: Event) {
  const target = event.currentTarget as HTMLElement;
  window.clearTimeout(timer);
  if (performance.now() - closedAt < WARM_MS || anchor) {
    show(target);
  } else {
    timer = window.setTimeout(() => show(target), DELAY_MS);
  }
}

function leave(event: Event) {
  if (event.currentTarget === anchor || !anchor) hide();
}

export const vTip: Directive<HTMLElement, string | null | undefined> = {
  mounted(el, binding) {
    texts.set(el, binding.value ?? '');
    el.addEventListener('mouseenter', enter);
    el.addEventListener('mouseleave', leave);
    el.addEventListener('mousedown', hide);
  },
  updated(el, binding) {
    const text = binding.value ?? '';
    if (texts.get(el) === text) return;
    texts.set(el, text);
    if (anchor !== el) return;
    if (!text) {
      hide();
      return;
    }
    render(text);
    place(el);
  },
  beforeUnmount(el) {
    if (anchor === el) hide();
    el.removeEventListener('mouseenter', enter);
    el.removeEventListener('mouseleave', leave);
    el.removeEventListener('mousedown', hide);
  },
};

// The bar hiding, or the window losing the pointer, takes any tooltip with it.
window.addEventListener('blur', hide);
document.addEventListener('mouseleave', hide);
