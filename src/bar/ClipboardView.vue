<script setup lang="ts">
  import { vFit } from '../fit';
  import { computed, nextTick, onMounted, ref, watch } from 'vue';
  import {
    clipClear,
    clipCopy,
    clipDelete,
    clipDetail,
    clipList,
    clipPaste,
    clipPin,
    errorMessage,
  } from '../api';
  import type { ClipDetail, ClipKind, ClipSummary } from '../types';
  import { dragGesture, dragOut } from './dragOut';
  import { bytes, dateTime } from '../format';
  import { isMac, MOD_KEY as mod } from '../keys';
  import SelectMenu from '../components/SelectMenu.vue';

  const props = defineProps<{ initialQuery: string }>();
  // `picked` means an item was pasted or copied, which ends the clipboard picker.
  const emit = defineEmits<{ exit: []; picked: [] }>();

  interface Action {
    label: string;
    keys?: string;
    run: () => unknown;
  }

  const FILTERS: { id: string; label: string }[] = [
    { id: 'all', label: 'All types' },
    { id: 'text', label: 'Text' },
    { id: 'link', label: 'Links' },
    { id: 'color', label: 'Colors' },
    { id: 'image', label: 'Images' },
    { id: 'files', label: 'Files' },
    { id: 'pinned', label: 'Pinned' },
  ];

  const inputEl = ref<HTMLInputElement | null>(null);

  // Dragging an item out drops it into the other app: files as themselves, an image as a PNG
  // and text as a .txt file.
  const gesture = dragGesture((index) => {
    const item = items.value[index];
    if (!item) return;
    void dragOut({ clip: item.id }).catch((error) => (notice.value = errorMessage(error)));
  });
  const query = ref(props.initialQuery);
  const filter = ref('all');
  const items = ref<ClipSummary[]>([]);
  const problem = ref<string | null>(null);
  const enabled = ref(true);
  const selected = ref(0);
  const detail = ref<ClipDetail | null>(null);
  const notice = ref<string | null>(null);
  const actionsOpen = ref(false);
  const actionIndex = ref(0);
  const confirmClear = ref(false);

  let generation = 0;

  const current = computed(() => items.value[selected.value]);
  const pinnedCount = computed(() => items.value.filter((item) => item.pinned).length);

  const stats = computed(() => {
    const text = detail.value?.text ?? '';
    if (!detail.value || !['text', 'link', 'color'].includes(detail.value.kind)) return null;
    const words = text.trim() ? text.trim().split(/\s+/).length : 0;
    return { chars: text.length, words };
  });

  const actions = computed<Action[]>(() => {
    const item = current.value;
    if (!item) return [];
    const list: Action[] = [
      { label: 'Paste', keys: '↵', run: () => paste(false) },
      { label: 'Copy to clipboard', keys: `${mod}↵`, run: () => copy() },
    ];
    if (item.kind !== 'image') {
      list.push({ label: 'Paste as plain text', keys: '⇧↵', run: () => paste(true) });
    }
    list.push({
      label: item.pinned ? 'Unpin' : 'Pin',
      keys: `${mod}P`,
      run: () => togglePin(),
    });
    list.push({ label: 'Delete', keys: '⌃X', run: () => remove() });
    list.push({
      label: confirmClear.value
        ? 'Press again to delete all unpinned items'
        : 'Delete all unpinned',
      run: () => clearAll(),
    });
    return list;
  });

  onMounted(async () => {
    await refresh();
    await nextTick();
    inputEl.value?.focus();
  });

  watch([query, filter], () => {
    selected.value = 0;
    void refresh();
  });

  // Typing goes back to the search once a type is picked, not to the dropdown.
  watch(filter, () => void nextTick(() => inputEl.value?.focus()));

  /** Opened again while still the bar's view: fresh text and the latest copies. */
  async function reopen(text: string) {
    const unchanged = query.value === text;
    query.value = text;
    selected.value = 0;
    // A changed query refreshes through its watcher.
    if (unchanged) await refresh();
    await nextTick();
    inputEl.value?.focus();
  }

  watch(current, () => {
    confirmClear.value = false;
    void loadDetail();
  });

  async function refresh() {
    const ticket = ++generation;
    try {
      const response = await clipList(query.value, filter.value);
      if (ticket !== generation) return;
      items.value = response.items;
      problem.value = response.problem;
      enabled.value = response.enabled;
      if (selected.value >= items.value.length)
        selected.value = Math.max(0, items.value.length - 1);
      await loadDetail();
    } catch (error) {
      problem.value = errorMessage(error);
    }
  }

  async function loadDetail() {
    const item = current.value;
    if (!item) {
      detail.value = null;
      return;
    }
    try {
      const loaded = await clipDetail(item.id);
      if (current.value?.id === loaded.id) detail.value = loaded;
    } catch {
      detail.value = null;
    }
  }

  // Only the items that fit are shown (v-fit); arrows stay among them, and typing narrows.
  const shown = ref(Infinity);
  function move(delta: number) {
    const count = Math.min(items.value.length, shown.value);
    if (count === 0) return;
    selected.value = (Math.min(selected.value, count - 1) + delta + count) % count;
  }

  async function paste(plain: boolean) {
    const item = current.value;
    if (!item) return;
    try {
      const outcome = await clipPaste(item.id, plain);
      if (outcome === 'needsPermission') {
        notice.value = isMac
          ? 'Copied. To paste straight into apps, allow Zephyr in System Settings > Privacy & Security > Accessibility.'
          : 'Copied. Paste it with Ctrl+V.';
      } else {
        emit('picked');
      }
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  async function copy() {
    const item = current.value;
    if (!item) return;
    try {
      await clipCopy(item.id);
      emit('picked');
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  async function togglePin() {
    const item = current.value;
    if (!item) return;
    try {
      await clipPin(item.id, !item.pinned);
      await refresh();
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  async function remove() {
    const item = current.value;
    if (!item) return;
    try {
      await clipDelete(item.id);
      await refresh();
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  async function clearAll() {
    if (!confirmClear.value) {
      confirmClear.value = true;
      actionsOpen.value = true;
      return;
    }
    confirmClear.value = false;
    try {
      const removed = await clipClear(false);
      notice.value = `Deleted ${removed} items. Pinned items stayed.`;
      await refresh();
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  function runAction(action: Action | undefined) {
    if (!action) return;
    if (!action.label.startsWith('Delete all') && !action.label.startsWith('Press again')) {
      actionsOpen.value = false;
    }
    void action.run();
  }

  function cycleFilter(backward: boolean) {
    const index = FILTERS.findIndex((item) => item.id === filter.value);
    const next = (index + (backward ? -1 : 1) + FILTERS.length) % FILTERS.length;
    filter.value = FILTERS[next].id;
  }

  // The type filter opens the glass menu Settings uses, not the native white popup.
  const clipEl = ref<HTMLElement | null>(null);
  const filterMenu = ref<InstanceType<typeof SelectMenu> | null>(null);

  /** Called by the bar for every key while clipboard history is showing. */
  function onKey(event: KeyboardEvent) {
    // An open filter menu takes its own keys.
    if (filterMenu.value?.isOpen()) return;
    const modifier = event.metaKey || event.ctrlKey;
    if (modifier && event.code === 'KeyK') {
      event.preventDefault();
      actionsOpen.value = !actionsOpen.value && actions.value.length > 0;
      actionIndex.value = 0;
      return;
    }
    if (actionsOpen.value) {
      if (event.key === 'Escape') {
        event.preventDefault();
        actionsOpen.value = false;
        confirmClear.value = false;
      } else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
        event.preventDefault();
        const count = actions.value.length;
        actionIndex.value =
          (actionIndex.value + (event.key === 'ArrowDown' ? 1 : -1) + count) % count;
      } else if (event.key === 'Enter') {
        event.preventDefault();
        if (!event.repeat) runAction(actions.value[actionIndex.value]);
      }
      return;
    }
    if (event.key === 'Escape') {
      event.preventDefault();
      emit('exit');
      return;
    }
    if (event.key === 'Backspace' && !event.repeat && query.value === '') {
      event.preventDefault();
      emit('exit');
      return;
    }
    if (event.key === 'Tab') {
      event.preventDefault();
      cycleFilter(event.shiftKey);
      return;
    }
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      move(event.key === 'ArrowDown' ? 1 : -1);
      return;
    }
    if (event.key === 'Enter') {
      event.preventDefault();
      if (event.repeat) return;
      if (modifier) void copy();
      else void paste(event.shiftKey);
      return;
    }
    // Held down, these act once: a repeat would pin and unpin, or delete item after item.
    if (modifier && event.code === 'KeyP') {
      event.preventDefault();
      if (!event.repeat) void togglePin();
      return;
    }
    if (event.ctrlKey && event.code === 'KeyX' && !event.metaKey) {
      event.preventDefault();
      if (!event.repeat) void remove();
    }
  }

  function kindIcon(kind: ClipKind): string {
    return { text: '¶', link: '↗', color: '●', image: '▣', files: '▤' }[kind];
  }

  function ago(seconds: number): string {
    const diff = Math.max(0, Date.now() / 1000 - seconds);
    if (diff < 60) return 'now';
    if (diff < 3600) return `${Math.floor(diff / 60)}m`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h`;
    return `${Math.floor(diff / 86400)}d`;
  }

  defineExpose({ onKey, refresh, reopen });
</script>

<template>
  <div ref="clipEl" class="clip">
    <SelectMenu ref="filterMenu" :root="clipEl" />
    <div class="input-row">
      <svg class="mark" viewBox="0 0 24 24" aria-hidden="true">
        <rect x="6" y="4" width="12" height="16" rx="2" />
        <path d="M9 4h6v3H9z" />
      </svg>
      <input
        ref="inputEl"
        v-model="query"
        class="query"
        type="text"
        placeholder="Search clipboard history"
        spellcheck="false"
        autocomplete="off"
        autocapitalize="off"
      />
      <select v-model="filter" class="clip-filter" aria-label="Type" @mousedown.stop>
        <option v-for="option in FILTERS" :key="option.id" :value="option.id">
          {{ option.label }}
        </option>
      </select>
    </div>

    <p v-if="notice" class="notice">{{ notice }}</p>
    <p v-if="!enabled" class="empty">Clipboard history is off. Turn it on in Settings.</p>
    <p v-else-if="problem && items.length === 0" class="empty">{{ problem }}</p>
    <p v-else-if="items.length === 0" class="empty">
      {{ query || filter !== 'all' ? 'Nothing matches.' : 'Copy something and it shows up here.' }}
    </p>

    <div v-else class="clip-body">
      <ul
        v-fit="(count: number) => (shown = count)"
        class="clip-list"
        role="listbox"
        aria-label="Clipboard history"
      >
        <template v-for="(item, index) in items" :key="item.id">
          <li v-if="index === 0 && item.pinned" class="clip-group">Pinned</li>
          <li
            v-if="!item.pinned && (index === 0 || items[index - 1].pinned) && pinnedCount"
            class="clip-group"
          >
            Recent
          </li>
          <li
            :data-index="index"
            role="option"
            :aria-selected="index === selected"
            class="clip-item"
            :class="{ selected: index === selected }"
            @mousedown.prevent="gesture.down($event, index)"
            @mouseup="gesture.up()"
            @mousemove="gesture.move($event)"
            @click="selected = index"
            @dblclick="paste(false)"
          >
            <span
              class="clip-icon"
              :style="item.color ? { color: item.color } : undefined"
              aria-hidden="true"
              >{{ kindIcon(item.kind) }}</span
            >
            <span class="label">{{ item.title }}</span>
            <span class="hint">{{ item.pinned ? '📌' : ago(item.lastCopied) }}</span>
          </li>
        </template>
      </ul>

      <section class="clip-preview" aria-live="polite">
        <ul v-if="actionsOpen" class="results actions clip-actions" role="listbox">
          <li
            v-for="(action, index) in actions"
            :key="action.label"
            class="item"
            :class="{ selected: index === actionIndex }"
            @mousedown.prevent
            @click="runAction(action)"
            @mousemove="actionIndex = index"
          >
            <span class="label">{{ action.label }}</span>
            <span v-if="action.keys" class="hint">{{ action.keys }}</span>
          </li>
        </ul>
        <template v-else-if="detail">
          <div class="clip-content">
            <div v-if="detail.kind === 'color' && detail.color" class="clip-swatch">
              <span :style="{ background: detail.color }" />
              <code>{{ detail.text }}</code>
            </div>
            <img
              v-else-if="detail.kind === 'image' && detail.imageUrl"
              :src="detail.imageUrl"
              alt="Copied image"
              class="clip-image"
            />
            <ul v-else-if="detail.kind === 'files'" class="clip-files">
              <li v-for="file in detail.files" :key="file">{{ file }}</li>
            </ul>
            <pre v-else class="clip-text">{{ detail.text }}</pre>
            <details v-if="detail.ocr" class="clip-ocr">
              <summary>Text in image</summary>
              <pre class="clip-text">{{ detail.ocr }}</pre>
            </details>
          </div>
          <dl class="clip-meta">
            <template v-if="detail.source">
              <dt>Source</dt>
              <dd>{{ detail.source }}</dd>
            </template>
            <dt>Type</dt>
            <dd>{{ detail.kind[0].toUpperCase() + detail.kind.slice(1) }}</dd>
            <template v-if="stats">
              <dt>Characters</dt>
              <dd>{{ stats.chars.toLocaleString() }}</dd>
              <dt>Words</dt>
              <dd>{{ stats.words.toLocaleString() }}</dd>
            </template>
            <template v-if="detail.image">
              <dt>Dimensions</dt>
              <dd>{{ detail.image.width }}×{{ detail.image.height }}</dd>
              <dt>Size</dt>
              <dd>{{ bytes(detail.image.bytes) }}</dd>
            </template>
            <dt>Times copied</dt>
            <dd>{{ detail.copies }}</dd>
            <dt>Last copied</dt>
            <dd>{{ dateTime(detail.lastCopied) }}</dd>
            <dt>First copied</dt>
            <dd>{{ dateTime(detail.firstCopied) }}</dd>
          </dl>
        </template>
      </section>
    </div>

    <footer class="footer">
      <span>
        <template v-if="actionsOpen">↵ run · Esc back</template>
        <template v-else>
          ↵ paste · {{ mod }}↵ copy · ⇧↵ plain text · Tab type · {{ mod }}K actions
        </template>
      </span>
      <span>{{ items.length }} items</span>
    </footer>
  </div>
</template>
