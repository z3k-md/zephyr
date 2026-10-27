<script setup lang="ts">
  import { computed, ref, watch } from 'vue';
  import {
    bindingFailures,
    errorMessage,
    pauseShortcuts,
    pickFolder,
    saveBinding,
    saveLaunchGroups,
  } from '../api';
  import { captureShortcut, formatShortcut, isModifierKey } from '../keys';
  import { useWindowListener } from '../windowEvents';
  import Keys from '../components/Keys.vue';
  import RowIcon from '../bar/RowIcon.vue';
  import AppPicker from './AppPicker.vue';
  import type { Binding, GroupItem, LaunchGroup, Snapshot } from '../types';
  import InfoTip from '../components/InfoTip.vue';

  // Settings › Launch groups: named sets of apps (and links and folders) the bar opens in
  // one go. Each group has a name, which the bar finds it by, an optional hotkey, and its
  // apps in the order they open: added from a search that ranks them as the bar does, then
  // dragged (or Alt+↑/↓) into order.
  const props = defineProps<{ snapshot: Snapshot }>();
  const emit = defineEmits<{ apply: [snapshot: Snapshot] }>();

  const error = ref<string | null>(null);
  const busy = ref(false);
  const groups = computed(() => props.snapshot.launchGroups ?? []);

  // Edits build on the last one sent, not the last one saved, and save in order, so a rename
  // followed at once by an added app keeps both.
  let draft: LaunchGroup[] | null = null;
  let saving: Promise<void> = Promise.resolve();

  function save(next: LaunchGroup[]) {
    draft = next;
    busy.value = true;
    error.value = null;
    saving = saving
      .then(async () => emit('apply', await saveLaunchGroups(next)))
      .catch((err) => {
        error.value = errorMessage(err);
      })
      .finally(() => {
        if (draft === next) {
          draft = null;
          busy.value = false;
        }
      });
  }

  function update(id: string, change: (group: LaunchGroup) => LaunchGroup) {
    save((draft ?? groups.value).map((group) => (group.id === id ? change(group) : group)));
  }

  function addGroup() {
    const id = `group-${Date.now().toString(36)}`;
    const current = draft ?? groups.value;
    save([...current, { id, name: `Group ${current.length + 1}`, icon: '', items: [] }]);
  }

  function remove(id: string) {
    save((draft ?? groups.value).filter((group) => group.id !== id));
  }

  function addItem(id: string, item: GroupItem) {
    update(id, (group) => ({ ...group, items: [...group.items, item] }));
  }

  /** The apps a group already has, which its search leaves out. */
  function appIds(group: LaunchGroup): string[] {
    return group.items.filter((item) => item.kind === 'app').map((item) => item.value);
  }

  // A link is added from a small field that opens under the group's search.
  const linking = ref<string | null>(null);
  const link = ref('');
  function addUrl(id: string) {
    const url = link.value.trim();
    if (!url) return;
    const value = /^https?:\/\//.test(url) ? url : `https://${url}`;
    link.value = '';
    linking.value = null;
    addItem(id, { kind: 'url', value, label: value.replace(/^https?:\/\//, '') });
  }

  async function addFolder(id: string) {
    const folder = await pickFolder().catch(() => null);
    if (!folder) return;
    addItem(id, { kind: 'folder', value: folder, label: folder.split(/[\\/]/).pop() || folder });
  }

  function move(id: string, from: number, to: number) {
    update(id, (group) => {
      const items = [...group.items];
      const [item] = items.splice(from, 1);
      items.splice(Math.max(0, Math.min(items.length, to)), 0, item);
      return { ...group, items };
    });
  }

  function removeItem(id: string, index: number) {
    update(id, (group) => ({ ...group, items: group.items.filter((_, at) => at !== index) }));
  }

  function itemIcon(item: GroupItem): string {
    if (item.kind === 'app') return `app:${item.value}`;
    return item.kind === 'url' ? 'glyph:link' : 'glyph:folder';
  }

  // Dragging a row onto another puts it there.
  const dragging = ref<{ group: string; index: number } | null>(null);
  const over = ref<number | null>(null);
  function drop(group: string, index: number) {
    const from = dragging.value;
    dragging.value = null;
    over.value = null;
    if (from && from.group === group && from.index !== index) move(group, from.index, index);
  }

  /** On a focused row: Alt+↑/↓ moves it, Delete or Backspace takes it out. */
  function onRowKey(event: KeyboardEvent, group: LaunchGroup, index: number) {
    if (event.altKey && (event.key === 'ArrowUp' || event.key === 'ArrowDown')) {
      event.preventDefault();
      const to = index + (event.key === 'ArrowDown' ? 1 : -1);
      if (to < 0 || to >= group.items.length) return;
      move(group.id, index, to);
      void focusRow(group.id, to);
    } else if (event.key === 'Delete' || event.key === 'Backspace') {
      event.preventDefault();
      removeItem(group.id, index);
    }
  }

  async function focusRow(group: string, index: number) {
    await saving;
    document.querySelector<HTMLElement>(`[data-row="${group}:${index}"]`)?.focus();
  }

  // Each group's hotkey is its binding in Settings › Shortcuts, recorded right here.
  const listening = ref<string | null>(null);
  watch(
    () => listening.value !== null,
    (recording) => void pauseShortcuts(recording).catch(() => undefined)
  );
  const failures = ref<string[]>([]);
  void bindingFailures()
    .then((found) => (failures.value = found))
    .catch(() => undefined);

  function bindingOf(group: LaunchGroup): Binding | undefined {
    return props.snapshot.bindings?.find((binding) => binding.target === `group:${group.id}`);
  }

  function hotkeyOf(group: LaunchGroup): string {
    return bindingOf(group)?.hotkey ?? '';
  }

  async function saveHotkey(group: LaunchGroup, hotkey: string) {
    const saved = bindingOf(group);
    try {
      emit(
        'apply',
        await saveBinding({
          target: `group:${group.id}`,
          label: group.name,
          hotkey,
          alias: saved?.alias ?? '',
          direct: saved?.direct ?? false,
        })
      );
      failures.value = await bindingFailures().catch(() => []);
      error.value = null;
    } catch (err) {
      error.value = errorMessage(err);
    }
  }

  function onKey(event: KeyboardEvent) {
    const id = listening.value;
    if (!id || event.isComposing) return;
    event.preventDefault();
    event.stopPropagation();
    const group = groups.value.find((item) => item.id === id);
    if (event.key === 'Escape' || !group) {
      listening.value = null;
      return;
    }
    const hotkey = captureShortcut(event, true);
    if (!hotkey) {
      if (!isModifierKey(event)) error.value = 'Use at least one modifier and a key.';
      return;
    }
    listening.value = null;
    void saveHotkey(group, hotkey);
  }

  useWindowListener('keydown', onKey, true);
</script>

<template>
  <section id="section-launchgroups">
    <h2>
      Launch groups
      <InfoTip
        text="Open several apps at once by typing a group's name in the bar or pressing its hotkey. Apps already open come to the front."
      />
    </h2>
    <p v-if="error" class="notice">{{ error }}</p>
    <article v-for="group in groups" :key="group.id" class="launch-group">
      <div class="launch-group-head">
        <input
          class="launch-group-name"
          :value="group.name"
          type="text"
          maxlength="40"
          aria-label="Group name"
          spellcheck="false"
          @change="
            update(group.id, (g) => ({ ...g, name: ($event.target as HTMLInputElement).value }))
          "
        />
        <button
          type="button"
          class="recorder"
          :class="{ listening: listening === group.id }"
          title="Hotkey"
          @click="listening = group.id"
          @blur="listening === group.id && (listening = null)"
        >
          <template v-if="listening === group.id">Press a shortcut</template>
          <Keys v-else-if="hotkeyOf(group)" :keys="hotkeyOf(group)" />
          <template v-else>Add hotkey</template>
        </button>
        <button
          v-if="hotkeyOf(group)"
          type="button"
          class="text-button"
          @click="saveHotkey(group, '')"
        >
          Clear
        </button>
        <span class="spacer" />
        <button type="button" class="text-button" :disabled="busy" @click="remove(group.id)">
          Delete group
        </button>
      </div>
      <p v-if="failures.includes(`group:${group.id}`)" class="notice">
        Another app already uses {{ formatShortcut(hotkeyOf(group)) }}. Pick another.
      </p>

      <ol v-if="group.items.length" class="launch-items">
        <li
          v-for="(item, index) in group.items"
          :key="`${item.kind}:${item.value}`"
          :data-row="`${group.id}:${index}`"
          class="launch-item"
          :class="{
            dragging: dragging?.group === group.id && dragging.index === index,
            over: dragging?.group === group.id && over === index,
          }"
          tabindex="0"
          draggable="true"
          :aria-label="`${item.label}. Alt+Up or Alt+Down moves it, Delete removes it.`"
          @dragstart="dragging = { group: group.id, index }"
          @dragend="
            dragging = null;
            over = null;
          "
          @dragover.prevent="over = index"
          @drop.prevent="drop(group.id, index)"
          @keydown="onRowKey($event, group, index)"
        >
          <svg class="launch-grip" viewBox="0 0 24 24" aria-hidden="true">
            <path d="M9 6h.01M15 6h.01M9 12h.01M15 12h.01M9 18h.01M15 18h.01" />
          </svg>
          <RowIcon :icon="itemIcon(item)" :label="item.label" :size="20" />
          <span class="launch-item-label">{{ item.label }}</span>
          <span v-if="item.kind !== 'app'" class="hint-text">{{
            item.kind === 'url' ? 'Link' : 'Folder'
          }}</span>
          <button
            type="button"
            class="launch-item-remove"
            :aria-label="`Remove ${item.label}`"
            title="Remove"
            @click="removeItem(group.id, index)"
          >
            <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M7 7l10 10M17 7 7 17" /></svg>
          </button>
        </li>
      </ol>

      <AppPicker
        :exclude="appIds(group)"
        @pick="(id, name) => addItem(group.id, { kind: 'app', value: id, label: name })"
      />
      <form v-if="linking === group.id" class="launch-link" @submit.prevent="addUrl(group.id)">
        <input
          v-model="link"
          type="text"
          placeholder="https://"
          spellcheck="false"
          aria-label="Link"
          @keydown.esc.stop="linking = null"
        />
        <button type="submit">Add</button>
      </form>
      <div class="launch-more">
        <button
          type="button"
          class="text-button"
          @click="linking = linking === group.id ? null : group.id"
        >
          Add a link
        </button>
        <button type="button" class="text-button" :disabled="busy" @click="addFolder(group.id)">
          Add a folder…
        </button>
      </div>
    </article>
    <div class="field inline">
      <button type="button" class="primary" :disabled="busy" @click="addGroup">New group</button>
    </div>
  </section>
</template>
