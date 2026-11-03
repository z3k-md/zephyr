<script setup lang="ts">
  import { computed, nextTick, onMounted, ref, watch } from 'vue';
  import KeyHints from '../components/KeyHints.vue';
  import { errorMessage, pasteText, saveEmojiPrefs } from '../api';
  import { isMac } from '../keys';
  import { loadCatalog, type Catalog } from './emoji/catalog';
  import { EMOJI_COLUMNS, move, place, type GridKey } from './emoji/nav';
  import { emojiSections, RECENT_LIMIT, type EmojiItem } from './emoji/search';
  import { clampSkin, codePointLabel, numericEntity, withSkin } from './emoji/skin';

  const props = defineProps<{ query: string; skin: number; recents?: string[] }>();
  const emit = defineEmits<{ notice: [message: string] }>();

  const HINTS = [
    { keys: 'enter', label: 'paste' },
    { keys: 'mod+enter', label: 'copy' },
    { keys: 'tab', label: 'skin tone' },
  ];

  const KEYS: Record<string, GridKey> = {
    ArrowLeft: 'left',
    ArrowRight: 'right',
    ArrowUp: 'up',
    ArrowDown: 'down',
    PageUp: 'pageup',
    PageDown: 'pagedown',
  };

  const catalog = ref<Catalog | null>(null);
  const problem = ref<string | null>(null);
  const skinTone = ref(clampSkin(props.skin));
  const recent = ref<string[]>([]);
  const selected = ref(0);
  const scrollEl = ref<HTMLElement | null>(null);
  // State can arrive after the grid opens. A Tab or a paste before that is kept and written
  // once the saved recents are known, so an early pick doesn't replace them with nothing.
  let touched = false;
  let synced = false;

  watch(
    () => props.skin,
    (value) => {
      if (!touched) skinTone.value = clampSkin(value);
    }
  );
  watch(
    () => props.recents,
    (value) => {
      if (!value) return;
      if (!synced) {
        synced = true;
        const picked = recent.value;
        recent.value = [...picked, ...value.filter((char) => !picked.includes(char))].slice(
          0,
          RECENT_LIMIT
        );
        if (touched) void persist();
        return;
      }
      if (!touched) recent.value = value.slice(0, RECENT_LIMIT);
    },
    { immediate: true }
  );

  const sections = computed(() =>
    catalog.value ? emojiSections(catalog.value.items, props.query, recent.value) : []
  );
  const flat = computed(() => sections.value.flatMap((section) => section.items));
  const cells = computed(() =>
    place(
      sections.value.map((section) => section.items.length),
      EMOJI_COLUMNS
    )
  );
  const current = computed(() => flat.value[selected.value] ?? null);
  const gridStyle = { gridTemplateColumns: `repeat(${EMOJI_COLUMNS}, minmax(0, 1fr))` };

  onMounted(() => {
    void loadCatalog()
      .then((loaded) => (catalog.value = loaded))
      .catch(() => (problem.value = "Emoji couldn't be loaded."));
  });

  watch(
    () => props.query,
    () => {
      selected.value = 0;
      if (scrollEl.value) scrollEl.value.scrollTop = 0;
    }
  );
  watch(flat, (items) => {
    if (selected.value >= items.length) selected.value = Math.max(0, items.length - 1);
  });
  watch(selected, () => void nextTick(() => reveal(selected.value)));

  function offset(sectionIndex: number): number {
    let total = 0;
    for (let i = 0; i < sectionIndex; i++) total += sections.value[i]?.items.length ?? 0;
    return total;
  }

  function shown(item: EmojiItem): string {
    if (!item.skin || skinTone.value === 0 || !catalog.value) return item.char;
    return withSkin(item.char, skinTone.value, catalog.value.bases);
  }

  function entityOf(item: EmojiItem): string {
    const glyph = shown(item);
    if (item.entity && glyph === item.char) return item.entity;
    return numericEntity(glyph);
  }

  function pageRows(): number {
    const scroller = scrollEl.value;
    const cell = scroller?.querySelector('.emoji-cell');
    const row = cell?.getBoundingClientRect().height || 36;
    const height = scroller?.clientHeight ?? row * 6;
    return Math.max(1, Math.floor(height / row));
  }

  function reveal(index: number) {
    const scroller = scrollEl.value;
    const cell = scroller?.querySelector(`[data-index="${index}"]`);
    if (!scroller || !(cell instanceof HTMLElement)) return;
    const box = scroller.getBoundingClientRect();
    const cellBox = cell.getBoundingClientRect();
    if (cellBox.top < box.top) scroller.scrollTop -= box.top - cellBox.top;
    else if (cellBox.bottom > box.bottom) scroller.scrollTop += cellBox.bottom - box.bottom;
  }

  let writing = false;
  let again = false;
  async function persist() {
    if (!synced) return;
    if (writing) {
      again = true;
      return;
    }
    writing = true;
    try {
      do {
        again = false;
        await saveEmojiPrefs(skinTone.value, recent.value.slice(0, RECENT_LIMIT));
      } while (again);
    } catch (error) {
      emit('notice', errorMessage(error));
    } finally {
      writing = false;
    }
  }

  function remember(char: string) {
    touched = true;
    recent.value = [char, ...recent.value.filter((item) => item !== char)].slice(0, RECENT_LIMIT);
    void persist();
  }

  function cycleSkin(step: number) {
    touched = true;
    skinTone.value = (skinTone.value + step + 6) % 6;
    void persist();
  }

  async function commit(copyOnly: boolean) {
    const item = current.value;
    if (!item) return;
    try {
      const outcome = await pasteText(shown(item), copyOnly);
      remember(item.char);
      if (outcome === 'needsPermission') {
        emit(
          'notice',
          isMac
            ? 'Copied. To paste straight into apps, allow Zephyr in System Settings > Privacy & Security > Accessibility.'
            : 'Copied. Paste it with Ctrl+V.'
        );
      }
    } catch (error) {
      emit('notice', errorMessage(error));
    }
  }

  /** Keys the grid handles while it is showing. False leaves the key to the bar. */
  function onKey(event: KeyboardEvent): boolean {
    const key = KEYS[event.key];
    if (key && !event.altKey && !event.ctrlKey && !event.metaKey && !event.shiftKey) {
      event.preventDefault();
      selected.value = move(cells.value, selected.value, key, pageRows());
      return true;
    }
    if (event.key === 'Tab' && !event.ctrlKey && !event.metaKey && !event.altKey) {
      event.preventDefault();
      if (!event.repeat) cycleSkin(event.shiftKey ? -1 : 1);
      return true;
    }
    if (event.key === 'Enter') {
      event.preventDefault();
      if (event.repeat || event.altKey) return true;
      const modifier = event.metaKey || event.ctrlKey;
      if (event.shiftKey && !modifier) return true;
      void commit(modifier);
      return true;
    }
    return false;
  }

  defineExpose({ onKey });
</script>

<template>
  <div class="emoji-panel">
    <div ref="scrollEl" class="emoji-scroll">
      <p v-if="problem" class="empty">{{ problem }}</p>
      <p v-else-if="!catalog" class="empty">Loading emoji…</p>
      <p v-else-if="flat.length === 0" class="empty">Nothing matches.</p>
      <div v-else class="emoji-grid" :style="gridStyle" role="grid" aria-label="Emoji and symbols">
        <template v-for="(section, sectionIndex) in sections" :key="section.kind + section.title">
          <h3 class="emoji-heading">{{ section.title }}</h3>
          <button
            v-for="(item, itemIndex) in section.items"
            :key="item.char"
            type="button"
            class="emoji-cell"
            :class="{ selected: offset(sectionIndex) + itemIndex === selected }"
            :data-index="offset(sectionIndex) + itemIndex"
            role="gridcell"
            :aria-selected="offset(sectionIndex) + itemIndex === selected"
            :aria-label="item.name"
            tabindex="-1"
            @mousedown.prevent="selected = offset(sectionIndex) + itemIndex"
            @dblclick="commit(false)"
          >
            {{ shown(item) }}
          </button>
        </template>
      </div>
    </div>
    <footer class="footer emoji-foot">
      <span v-if="current" class="emoji-meta">
        <b>{{ current.name }}</b>
        <span>{{ codePointLabel(shown(current)) }}</span>
        <span class="emoji-entity">{{ entityOf(current) }}</span>
      </span>
      <span v-else />
      <KeyHints :hints="HINTS" />
    </footer>
  </div>
</template>
