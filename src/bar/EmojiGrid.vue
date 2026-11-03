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
  // Saved skin and recents can arrive after the grid opens. A tone change and a pick are
  // tracked apart, so a paste before the save loads doesn't throw the tone away, and a
  // Tab doesn't throw the saved recents away.
  let skinTouched = false;
  let recentsTouched = false;
  let synced = false;

  watch(
    () => props.skin,
    (value) => {
      if (!skinTouched) skinTone.value = clampSkin(value);
    }
  );
  watch(
    () => props.recents,
    (value) => {
      if (!value) return;
      if (!synced) {
        synced = true;
        if (!skinTouched) skinTone.value = clampSkin(props.skin);
        if (recentsTouched) {
          const picked = recent.value;
          recent.value = [...picked, ...value.filter((char) => !picked.includes(char))].slice(
            0,
            RECENT_LIMIT
          );
        } else {
          recent.value = value.slice(0, RECENT_LIMIT);
        }
        if (skinTouched || recentsTouched) void persist();
        return;
      }
      if (!recentsTouched) recent.value = value.slice(0, RECENT_LIMIT);
    },
    { immediate: true }
  );

  interface GridCell {
    item: EmojiItem;
    index: number;
    glyph: string;
  }

  interface GridBlock {
    key: string;
    title: string;
    cells: GridCell[];
  }

  const sections = computed(() =>
    catalog.value ? emojiSections(catalog.value.items, props.query, recent.value) : []
  );

  function glyphOf(item: EmojiItem, tone: number): string {
    if (!item.skin || tone === 0) return item.char;
    return withSkin(item.char, tone, item.tones ?? []);
  }

  // Glyphs and indexes stay put until the catalog, the skin tone, or the visible rows
  // change. A button's memo is its glyph, its index, and whether it is the selection,
  // so an arrow key redraws two cells instead of the whole grid.
  const glyphs = computed(() => {
    const loaded = catalog.value;
    const tone = skinTone.value;
    const map = new Map<string, string>();
    if (!loaded) return map;
    for (const item of loaded.items) map.set(item.char, glyphOf(item, tone));
    return map;
  });
  const blocks = computed(() => {
    const map = glyphs.value;
    let index = 0;
    const result: GridBlock[] = [];
    for (const section of sections.value) {
      const row: GridCell[] = [];
      for (const item of section.items) {
        row.push({ item, index, glyph: map.get(item.char) ?? item.char });
        index += 1;
      }
      result.push({ key: section.kind + section.title, title: section.title, cells: row });
    }
    return result;
  });
  const flat = computed(() => blocks.value.flatMap((block) => block.cells));
  const cells = computed(() =>
    place(
      blocks.value.map((block) => block.cells.length),
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

  function entityOf(item: EmojiItem, glyph: string): string {
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
        const skin = skinTouched ? skinTone.value : clampSkin(props.skin);
        await saveEmojiPrefs(skin, recent.value.slice(0, RECENT_LIMIT));
      } while (again);
    } catch (error) {
      emit('notice', errorMessage(error));
    } finally {
      writing = false;
    }
  }

  function remember(char: string) {
    recentsTouched = true;
    recent.value = [char, ...recent.value.filter((item) => item !== char)].slice(0, RECENT_LIMIT);
    void persist();
  }

  function cycleSkin(step: number) {
    skinTouched = true;
    skinTone.value = (skinTone.value + step + 6) % 6;
    void persist();
  }

  function pick(event: MouseEvent) {
    const button = event.currentTarget;
    if (!(button instanceof HTMLElement)) return;
    const index = Number(button.dataset.index);
    if (!Number.isInteger(index)) return;
    selected.value = index;
  }

  function commitPick() {
    void commit(false);
  }

  async function commit(copyOnly: boolean) {
    const cell = current.value;
    if (!cell) return;
    try {
      const outcome = await pasteText(cell.glyph, copyOnly);
      remember(cell.item.char);
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
        <template v-for="block in blocks" :key="block.key">
          <h3 class="emoji-heading">{{ block.title }}</h3>
          <button
            v-for="cell in block.cells"
            :key="cell.item.char"
            v-memo="[cell.glyph, cell.index, cell.index === selected]"
            type="button"
            class="emoji-cell"
            :class="{ selected: cell.index === selected }"
            :data-index="cell.index"
            role="gridcell"
            :aria-selected="cell.index === selected"
            :aria-label="cell.item.name"
            tabindex="-1"
            @mousedown.prevent="pick"
            @dblclick="commitPick"
          >
            {{ cell.glyph }}
          </button>
        </template>
      </div>
    </div>
    <footer class="footer emoji-foot">
      <span v-if="current" class="emoji-meta">
        <b>{{ current.item.name }}</b>
        <span>{{ codePointLabel(current.glyph) }}</span>
        <span class="emoji-entity">{{ entityOf(current.item, current.glyph) }}</span>
      </span>
      <span v-else />
      <KeyHints :hints="HINTS" />
    </footer>
  </div>
</template>
