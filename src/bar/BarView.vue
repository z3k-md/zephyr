<script setup lang="ts">
  import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import {
    dispatch,
    errorMessage,
    formatShortcut,
    getSnapshot,
    hideBar,
    launchApp,
    openFile,
    openSetting,
    openSettings,
    revealFile,
    setBarHeight,
    suggest,
  } from '../api';
  import { isSnapshot, type Destination, type Snapshot, type Suggestion } from '../types';

  const root = ref<HTMLElement | null>(null);
  const inputEl = ref<HTMLInputElement | null>(null);
  const snapshot = ref<Snapshot | null>(null);
  const query = ref('');
  const armedId = ref('google');
  const items = ref<Suggestion[]>([]);
  const mode = ref<'search' | 'destinations' | 'recent'>('recent');
  const notice = ref<string | null>(null);
  const selected = ref(-1);
  const loadError = ref<string | null>(null);

  let generation = 0;
  let remoteApplied = false;
  let userMoved = false;
  let suggestTimer = 0;
  let unlistens: UnlistenFn[] = [];
  let observer: ResizeObserver | null = null;

  const strip = computed(() =>
    (snapshot.value?.destinations ?? [])
      .filter((destination) => destination.pinned && !destination.disabled)
      .slice(0, 8)
  );

  const armed = computed(() =>
    snapshot.value?.destinations.find((destination) => destination.id === armedId.value)
  );

  const selectedItem = computed(() =>
    selected.value >= 0 ? items.value[selected.value] : undefined
  );

  const enterLabel = computed(() => {
    const item = selectedItem.value;
    if (item?.kind === 'app' || item?.kind === 'setting' || item?.kind === 'file') {
      return `Open ${item.label}`;
    }
    return armed.value?.name ?? 'search';
  });

  const isMac = navigator.userAgent.includes('Mac');

  const shortcutLabel = computed(() =>
    snapshot.value
      ? formatShortcut(snapshot.value.summonShortcut)
      : formatShortcut(isMac ? 'command+space' : 'alt+space')
  );

  onMounted(async () => {
    try {
      snapshot.value = await getSnapshot();
      armedId.value = snapshot.value.defaultDestinationId;
    } catch (error) {
      loadError.value = errorMessage(error);
    }

    unlistens.push(
      await listen('bar-shown', () => {
        resetForSummon();
        void focusInput();
      })
    );
    unlistens.push(
      await listen('bar-focus', () => {
        void focusInput();
      })
    );
    unlistens.push(
      await listen<unknown>('state-changed', (event) => {
        if (!isSnapshot(event.payload)) return;
        snapshot.value = event.payload;
        if (
          !snapshot.value.destinations.some(
            (destination) => destination.id === armedId.value && !destination.disabled
          )
        ) {
          armedId.value = snapshot.value.defaultDestinationId;
        }
      })
    );

    window.addEventListener('keydown', onKey, true);
    if (root.value) {
      observer = new ResizeObserver(() => syncHeight());
      observer.observe(root.value);
      syncHeight();
    }
    await focusInput();
    scheduleSuggest();
  });

  onUnmounted(() => {
    window.removeEventListener('keydown', onKey, true);
    observer?.disconnect();
    window.clearTimeout(suggestTimer);
    for (const unlisten of unlistens) unlisten();
  });

  watch(query, () => {
    selected.value = -1;
    userMoved = false;
    notice.value = null;
    scheduleSuggest();
  });

  watch(armedId, () => {
    selected.value = -1;
    userMoved = false;
    scheduleSuggest();
  });

  function resetForSummon() {
    query.value = '';
    selected.value = -1;
    notice.value = null;
    armedId.value = snapshot.value?.defaultDestinationId ?? armedId.value;
  }

  async function focusInput() {
    await nextTick();
    inputEl.value?.focus();
    inputEl.value?.select();
  }

  function syncHeight() {
    if (!root.value) return;
    const height = Math.ceil(root.value.getBoundingClientRect().height);
    void setBarHeight(height).catch(() => undefined);
  }

  // Local results (apps, history) land on the keystroke; remote suggestions follow after a
  // pause and only append, so the row Enter would take never shifts under the user.
  function scheduleSuggest() {
    window.clearTimeout(suggestTimer);
    const current = ++generation;
    remoteApplied = false;
    void loadSuggestions(current, false);
    suggestTimer = window.setTimeout(() => {
      void loadSuggestions(current, true);
    }, 120);
  }

  async function loadSuggestions(current: number, includeRemote: boolean) {
    try {
      const response = await suggest(query.value, armedId.value, includeRemote);
      if (current !== generation || (!includeRemote && remoteApplied)) return;
      if (includeRemote) remoteApplied = true;
      items.value = response.items;
      mode.value = response.mode;
      notice.value = response.notice;
      if (!userMoved) selected.value = response.preselect ?? -1;
      await nextTick();
      syncHeight();
    } catch (error) {
      if (current !== generation) return;
      notice.value = errorMessage(error);
    }
  }

  function onKey(event: KeyboardEvent) {
    if (event.isComposing) return;

    if (event.key === 'Escape') {
      event.preventDefault();
      void hideBar();
      return;
    }

    if (event.key === 'Tab') {
      event.preventDefault();
      cycleArmed(event.shiftKey);
      return;
    }

    if (
      event.ctrlKey &&
      !event.altKey &&
      !event.metaKey &&
      !event.shiftKey &&
      event.code.startsWith('Digit')
    ) {
      const index = Number(event.code.slice(5)) - 1;
      if (index >= 0 && index < strip.value.length) {
        event.preventDefault();
        if (event.repeat) return;
        void sendTo(strip.value[index]);
      }
      return;
    }

    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      moveSelection(event.key === 'ArrowDown' ? 1 : -1);
      return;
    }

    // File actions: Ctrl+Enter shows the file in its folder, Ctrl+Shift+C copies its path.
    const file = selectedItem.value?.kind === 'file' ? selectedItem.value.path : undefined;
    const modifier = event.ctrlKey || event.metaKey;
    if (file && modifier && !event.altKey && event.key === 'Enter') {
      event.preventDefault();
      if (event.repeat) return;
      void reveal(file);
      return;
    }
    if (file && modifier && event.shiftKey && !event.altKey && event.code === 'KeyC') {
      event.preventDefault();
      void copyPath(file);
      return;
    }

    if (event.key === 'Enter') {
      event.preventDefault();
      if (event.repeat) return;
      void accept();
    }
  }

  function cycleArmed(backward: boolean) {
    const destinations = strip.value;
    if (destinations.length === 0) return;
    const index = destinations.findIndex((destination) => destination.id === armedId.value);
    const next = backward
      ? index <= 0
        ? destinations.length - 1
        : index - 1
      : (index + 1) % destinations.length;
    armedId.value = destinations[next].id;
  }

  function hover(index: number) {
    if (selected.value === index) return;
    userMoved = true;
    selected.value = index;
  }

  function moveSelection(delta: number) {
    if (items.value.length === 0) return;
    userMoved = true;
    if (selected.value < 0) {
      selected.value = delta > 0 ? 0 : items.value.length - 1;
      return;
    }
    selected.value = (selected.value + delta + items.value.length) % items.value.length;
  }

  function arm(destination: Destination) {
    armedId.value = destination.id;
    inputEl.value?.focus();
  }

  async function sendTo(destination: Destination) {
    if (!query.value.trim()) {
      armedId.value = destination.id;
      return;
    }
    await run(query.value, destination.id, false);
  }

  async function accept() {
    if (mode.value === 'destinations') {
      const item = items.value[selected.value >= 0 ? selected.value : 0];
      if (!item) return;
      armedId.value = item.destinationId;
      query.value = '';
      return;
    }

    const item = selected.value >= 0 ? items.value[selected.value] : undefined;
    if (item) {
      if (item.kind === 'app' && item.appId) {
        await launch(item.appId);
        return;
      }
      if (item.kind === 'setting' && item.settingId) {
        await openPage(item.settingId);
        return;
      }
      if (item.kind === 'file' && item.path) {
        await openPath(item.path);
        return;
      }
      if (item.kind === 'destination') {
        armedId.value = item.destinationId;
        query.value = '';
        return;
      }
      await run(item.query, item.destinationId, false);
      return;
    }

    await run(query.value, armedId.value, true);
  }

  function choose(item: Suggestion) {
    if (item.kind === 'app' && item.appId) {
      void launch(item.appId);
      return;
    }
    if (item.kind === 'setting' && item.settingId) {
      void openPage(item.settingId);
      return;
    }
    if (item.kind === 'file' && item.path) {
      void openPath(item.path);
      return;
    }
    if (item.kind === 'destination') {
      armedId.value = item.destinationId;
      query.value = '';
      inputEl.value?.focus();
      return;
    }
    void run(item.query, item.destinationId, false);
  }

  async function launch(appId: string) {
    try {
      await launchApp(appId);
      query.value = '';
      selected.value = -1;
      notice.value = null;
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  async function openPage(settingId: string) {
    try {
      await openSetting(settingId);
      query.value = '';
      selected.value = -1;
      notice.value = null;
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  async function openPath(path: string) {
    try {
      await openFile(path);
      query.value = '';
      selected.value = -1;
      notice.value = null;
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  async function reveal(path: string) {
    try {
      await revealFile(path);
      query.value = '';
      selected.value = -1;
      notice.value = null;
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  async function copyPath(path: string) {
    try {
      await navigator.clipboard.writeText(path);
      notice.value = 'Copied the path';
    } catch {
      notice.value = "Couldn't copy the path";
    }
  }

  async function run(text: string, destinationId: string, interpret: boolean) {
    try {
      const outcome = await dispatch(text, destinationId, interpret);
      if (
        outcome.kind === 'opened' ||
        outcome.kind === 'launched' ||
        outcome.kind === 'settingOpened' ||
        outcome.kind === 'fileOpened'
      ) {
        query.value = '';
        selected.value = -1;
        notice.value = null;
        return;
      }
      if (outcome.kind === 'armed') {
        armedId.value = outcome.destinationId;
        query.value = '';
        notice.value = null;
        return;
      }
      if (outcome.kind === 'unknownBang') {
        notice.value = `No destination !${outcome.trigger}`;
        return;
      }
      notice.value = null;
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }
</script>

<template>
  <div ref="root" class="bar" role="dialog" aria-label="Zephyr">
    <div class="input-row">
      <svg class="mark" viewBox="0 0 24 24" aria-hidden="true">
        <circle cx="11" cy="11" r="6.5" />
        <path d="M16 16.5 20 20.5" />
      </svg>
      <input
        ref="inputEl"
        v-model="query"
        class="query"
        type="text"
        role="combobox"
        aria-autocomplete="list"
        aria-controls="results"
        :aria-expanded="items.length > 0"
        :aria-activedescendant="selected >= 0 ? `result-${selected}` : undefined"
        placeholder="Search, open an app, or ! for a destination"
        spellcheck="false"
        autocomplete="off"
        autocapitalize="off"
      />
      <button class="text-button" type="button" @mousedown.prevent @click="openSettings">
        Settings
      </button>
    </div>

    <div v-if="strip.length" class="strip" aria-label="Destinations">
      <button
        v-for="(destination, index) in strip"
        :key="destination.id"
        type="button"
        class="chip"
        :class="{ active: destination.id === armedId }"
        @mousedown.prevent
        @click="arm(destination)"
      >
        {{ destination.name }}
        <kbd>Ctrl+{{ index + 1 }}</kbd>
      </button>
    </div>

    <p v-if="loadError || notice" class="notice">{{ loadError || notice }}</p>

    <ul v-if="items.length" id="results" class="results" role="listbox">
      <li
        v-for="(item, index) in items"
        :id="`result-${index}`"
        :key="`${item.kind}-${item.destinationId}-${item.path ?? item.label}`"
        role="option"
        :aria-selected="index === selected"
        class="item"
        :class="{ selected: index === selected }"
        @mousedown.prevent
        @click="choose(item)"
        @mousemove="hover(index)"
      >
        <span class="label">{{ item.label }}</span>
        <span class="hint">{{ item.hint }}</span>
      </li>
    </ul>

    <p v-else-if="mode === 'recent' && !query.trim()" class="empty">
      Searches you run will show up here.
    </p>

    <footer class="footer">
      <span>
        Enter {{ enterLabel }}
        <template v-if="selectedItem?.kind === 'file'">
          · Ctrl+Enter show in {{ isMac ? 'Finder' : 'Explorer' }} · Ctrl+Shift+C copy path
        </template>
      </span>
      <span>{{ shortcutLabel }}</span>
    </footer>
  </div>
</template>
