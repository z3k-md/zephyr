/** On a focused row: Delete or Backspace takes it out. */ function onRowKey(event: KeyboardEvent,
group: LaunchGroup, index: number) { if (event.key === 'Delete' || event.key === 'Backspace') {
<script setup lang="ts">
  import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
  import { bindingFailures, errorMessage, pickFolder, saveBinding, saveLaunchGroups } from '../api';
  import { formatShortcut } from '../keys';
  import ShortcutRecorder from '../components/ShortcutRecorder.vue';
  import RowIcon from '../bar/RowIcon.vue';
  import AppPicker from './AppPicker.vue';
  import type { Binding, GroupItem, LaunchGroup, Snapshot } from '../types';
  import InfoTip from '../components/InfoTip.vue';

  // Settings › Launch groups: named sets of apps (and links and folders) the bar opens in
  // one go. Each group has a name, which the bar finds it by, an optional hotkey, and its
  // apps, added from a search that ranks them as the bar does. A group left with nothing in it
  // is dropped when the page is left.
  /** `shown`: the page is on screen; leaving it drops empty groups. */
  const props = defineProps<{ snapshot: Snapshot; shown: boolean }>();
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

  // A group nothing was added to (New group, then left alone) isn't kept: it goes when the
  // page is left, or when Settings is closed or hidden.
  function dropEmpty() {
    const current = draft ?? groups.value;
    const kept = current.filter((group) => group.items.length > 0);
    if (kept.length !== current.length) save(kept);
  }
  watch(
    () => props.shown,
    (shown) => {
      if (!shown) dropEmpty();
    }
  );
  function onHidden() {
    if (document.visibilityState === 'hidden') dropEmpty();
  }
  onMounted(() => document.addEventListener('visibilitychange', onHidden));
  onUnmounted(() => {
    document.removeEventListener('visibilitychange', onHidden);
    dropEmpty();
  });

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

  function removeItem(id: string, index: number) {
    update(id, (group) => ({ ...group, items: group.items.filter((_, at) => at !== index) }));
  }

  function itemIcon(item: GroupItem): string {
    if (item.kind === 'app') return `app:${item.value}`;
    return item.kind === 'url' ? 'glyph:link' : 'glyph:folder';
  }

  /** On a focused row: Delete or Backspace takes it out. */
  function onRowKey(event: KeyboardEvent, group: LaunchGroup, index: number) {
    if (event.key === 'Delete' || event.key === 'Backspace') {
      event.preventDefault();
      removeItem(group.id, index);
    }
  }

  // Each group's hotkey is its binding in Settings › Shortcuts, recorded right here.
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
        <ShortcutRecorder
          :keys="hotkeyOf(group)"
          function-keys
          aria-label="Hotkey"
          @record="saveHotkey(group, $event)"
          @invalid="error = $event"
        />
        <button
          v-if="hotkeyOf(group)"
          type="button"
          class="launch-button"
          @click="saveHotkey(group, '')"
        >
          Clear
        </button>
        <span class="spacer" />
        <button
          type="button"
          class="launch-button danger launch-delete"
          :disabled="busy"
          :aria-label="`Delete ${group.name}`"
          title="Delete group"
          @click="remove(group.id)"
        >
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M7 7l10 10M17 7 7 17" /></svg>
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
          tabindex="0"
          :aria-label="`${item.label}. Delete removes it.`"
          @keydown="onRowKey($event, group, index)"
        >
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
          class="launch-button"
          @click="linking = linking === group.id ? null : group.id"
        >
          Add a link
        </button>
        <button type="button" class="launch-button" :disabled="busy" @click="addFolder(group.id)">
          Add a folder…
        </button>
      </div>
    </article>
    <div class="field inline">
      <button type="button" class="primary" :disabled="busy" @click="addGroup">New group</button>
    </div>
  </section>
</template>
