<script setup lang="ts">
  import './clipcycle.css';
  import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { clipDetail } from '../api';
  import { GLYPHS } from '../bar/glyphs';
  import { elapsed } from '../format';
  import type { ClipCycleDone, ClipCycleShown, ClipDetail, ClipKind, ClipSummary } from '../types';
  import { visibleRange } from './window';

  // Rust owns the keys and the window's show/hide. This page never takes focus and gets no
  // key input; it only renders the events the hook emits.

  /** Newest row reads "just copied" when it landed within about this long. */
  const JUST_COPIED_SECONDS = 10;

  // Text and images have no row glyph of their own. Link and files reuse the bar's.
  const LINE: Partial<Record<ClipKind, string>> = {
    text: '<path d="M5 6h14M5 12h14M5 18h9"/>',
    link: GLYPHS.link,
    files: GLYPHS.folder,
    image:
      '<rect x="4" y="5.5" width="16" height="13" rx="2"/><circle cx="9" cy="10" r="1.3"/><path d="m4 16 4.5-3.5 3 2.5 2.5-2 6 4.5"/>',
    color: GLYPHS.eyedropper,
  };

  const shown = ref<ClipCycleShown | null>(null);
  const done = ref<ClipCycleDone | null>(null);
  /** Full text or thumbnail for the selected id. The window stays alive while hidden. */
  const detail = ref<ClipDetail | null>(null);
  const cache = new Map<number, ClipDetail>();
  let unlisten: UnlistenFn | null = null;

  document.documentElement.classList.add('clipcycle-page');

  onMounted(async () => {
    const stops = await Promise.all([
      listen<ClipCycleShown>('clipcycle-show', (event) => {
        done.value = null;
        shown.value = event.payload;
      }),
      listen<ClipCycleDone>('clipcycle-done', (event) => {
        shown.value = null;
        detail.value = null;
        done.value = event.payload;
      }),
      listen('clipcycle-cancel', () => {
        shown.value = null;
        done.value = null;
        detail.value = null;
      }),
    ]);
    unlisten = () => stops.forEach((stop) => stop());
  });

  onUnmounted(() => unlisten?.());

  /** `at` is only meaningful inside the list; a stale index still highlights a row. */
  function clampedAt(state: ClipCycleShown): number {
    if (!state.items.length) return -1;
    const raw = Math.trunc(state.at);
    const index = Number.isFinite(raw) ? raw : 0;
    return Math.min(state.items.length - 1, Math.max(0, index));
  }

  watch(
    () => {
      const state = shown.value;
      if (!state) return null;
      const at = clampedAt(state);
      return at < 0 ? null : (state.items[at]?.id ?? null);
    },
    (id) => {
      if (id == null) {
        detail.value = null;
        return;
      }
      const hit = cache.get(id);
      if (hit) {
        detail.value = hit;
        return;
      }
      // Drop the previous row's text immediately; a slow detail must not paint over the new one.
      detail.value = null;
      void clipDetail(id)
        .then((next) => {
          cache.set(next.id, next);
          const state = shown.value;
          if (state && state.items[clampedAt(state)]?.id === next.id) detail.value = next;
        })
        .catch(() => undefined);
    }
  );

  /** "12 m", "2 h": `elapsed` says "min", and the row is too narrow for it. */
  function ageOf(at: number): string {
    return elapsed(Math.max(0, Date.now() / 1000 - at)).replace(' min', ' m');
  }

  function metaOf(item: ClipSummary): string {
    const age = ageOf(item.lastCopied);
    return item.source ? `${item.source} · ${age}` : age;
  }

  /** The row draws at most three lines, so the rest of a long copy stays out of the DOM. */
  function preview(text: string, expanded: boolean): { text: string; multi: boolean } {
    const lines = text.split('\n');
    if (!expanded || lines.length < 2) return { text: lines[0] ?? '', multi: false };
    return { text: lines.slice(0, 3).join('\n'), multi: true };
  }

  function bodyOf(item: ClipSummary, full: ClipDetail | null): string {
    if (!full || full.id !== item.id) return item.title;
    if (item.kind === 'files' && full.files.length) return full.files.join('\n');
    if ((item.kind === 'text' || item.kind === 'link') && full.text) return full.text;
    return item.title;
  }

  const position = computed(() => {
    const state = shown.value;
    if (!state?.items.length) return '';
    return `${clampedAt(state) + 1} of ${state.items.length}`;
  });

  const rows = computed(() => {
    const state = shown.value;
    if (!state?.items.length) return [];
    const at = clampedAt(state);
    const { start, end } = visibleRange(state.items.length, at);
    const loaded = detail.value;
    const list = state.items.slice(start, end).map((item, offset) => {
      const index = start + offset;
      const on = index === at;
      const full = on ? loaded : null;
      const drawn = preview(bodyOf(item, full), on);
      const swatch = item.kind === 'color' ? (item.color ?? full?.color ?? null) : null;
      const thumb = item.kind === 'image' && on ? (full?.imageUrl ?? null) : null;
      return {
        item,
        on,
        fresh: index === 0 && Date.now() / 1000 - item.lastCopied <= JUST_COPIED_SECONDS,
        meta: metaOf(item),
        text: drawn.text,
        multi: drawn.multi,
        mono: drawn.multi && item.kind === 'text',
        swatch,
        thumb,
        glyph: swatch || thumb ? null : (LINE[item.kind] ?? null),
      };
    });
    // Newest at the bottom, so Up (a larger index) moves the highlight up the card.
    list.reverse();
    return list;
  });
</script>

<template>
  <div v-if="shown && rows.length" class="cyc" role="listbox" aria-label="Clipboard history">
    <div class="cyc-head">
      <span>Clipboard</span>
      <span class="grow" />
      <span>{{ position }}</span>
    </div>
    <div class="cyc-list">
      <div
        v-for="row in rows"
        :key="row.item.id"
        class="row"
        :class="{ on: row.on, img: row.on && row.item.kind === 'image' }"
        role="option"
        :aria-selected="row.on"
      >
        <span
          class="ic"
          :class="{ swatch: row.swatch }"
          :style="row.swatch ? { '--swatch': row.swatch } : undefined"
          aria-hidden="true"
        >
          <img v-if="row.thumb" :src="row.thumb" alt="" />
          <svg v-else-if="row.glyph" viewBox="0 0 24 24" v-html="row.glyph" />
        </span>
        <span class="t">
          <div class="title" :class="{ multi: row.multi, mono: row.mono }">{{ row.text }}</div>
        </span>
        <span class="meta">
          <span v-if="row.fresh" class="now">just copied</span>
          <template v-else>{{ row.meta }}</template>
        </span>
      </div>
    </div>
    <div class="cyc-foot">
      <span><b>↑↓</b> older / newer</span>
      <span><b>Release Ctrl</b> copies</span>
      <span><b>V</b> pastes</span>
      <span><b>Esc</b> cancels</span>
    </div>
  </div>
  <div v-if="done" class="toast" role="status">
    <span class="msg">{{ done.pasted ? 'Pasted' : 'Copied' }} “{{ done.title }}”</span>
    <span v-if="!done.pasted" class="hint">· Ctrl+V to paste</span>
  </div>
</template>
