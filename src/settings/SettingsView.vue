<script setup lang="ts">
  import { computed, nextTick, onMounted, onUnmounted, ref } from 'vue';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import {
    checkForUpdates,
    clearHistory,
    errorMessage,
    formatShortcut,
    getSnapshot,
    moveDestination,
    removeDestination,
    saveDestination,
    saveSettings,
    showBar,
  } from '../api';
  import { isSnapshot, type Destination, type Snapshot, type SuggestKind } from '../types';

  const snapshot = ref<Snapshot | null>(null);
  const status = ref<string | null>(null);
  const formError = ref<string | null>(null);
  const listening = ref(false);
  const editingId = ref<string | null>(null);
  const updateMessage = ref<string | null>(null);
  const busy = ref(false);

  const draft = ref({
    name: '',
    triggersText: '',
    urlTemplate: '',
    suggest: 'none' as SuggestKind,
  });

  const added = ref({
    name: '',
    triggersText: '',
    urlTemplate: 'https://example.com/search?q={query}',
    suggest: 'none' as SuggestKind,
  });

  let unlistens: UnlistenFn[] = [];

  const historyPreview = computed(() => snapshot.value?.history.slice(0, 8) ?? []);

  onMounted(async () => {
    await refresh();
    unlistens.push(
      await listen<unknown>('state-changed', (event) => {
        if (isSnapshot(event.payload)) snapshot.value = event.payload;
      })
    );
    // The bar's !set opens a specific section: by URL when it creates this window, by event
    // when the window already exists.
    unlistens.push(
      await listen<string>('settings-section', (event) => {
        void revealSection(event.payload);
      })
    );
    const section = new URLSearchParams(window.location.search).get('section');
    if (section) void revealSection(section);
    window.addEventListener('keydown', onShortcutKey, true);
  });

  onUnmounted(() => {
    window.removeEventListener('keydown', onShortcutKey, true);
    for (const unlisten of unlistens) unlisten();
  });

  async function revealSection(section: string) {
    await nextTick();
    const target = document.getElementById(`section-${section}`);
    if (!target) return;
    const still = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    target.scrollIntoView({ block: 'start', behavior: still ? 'auto' : 'smooth' });
    target.classList.remove('revealed');
    void target.offsetWidth;
    target.classList.add('revealed');
  }

  async function refresh() {
    try {
      snapshot.value = await getSnapshot();
    } catch (error) {
      formError.value = errorMessage(error);
    }
  }

  function apply(next: Snapshot) {
    snapshot.value = next;
    formError.value = null;
  }

  async function toggleStartup() {
    if (!snapshot.value) return;
    busy.value = true;
    try {
      apply(
        await saveSettings(
          snapshot.value.summonShortcut,
          !snapshot.value.launchAtStartup,
          snapshot.value.defaultDestinationId
        )
      );
    } catch (error) {
      formError.value = errorMessage(error);
    } finally {
      busy.value = false;
    }
  }

  async function changeDefault(event: Event) {
    if (!snapshot.value) return;
    const value = (event.target as HTMLSelectElement).value;
    busy.value = true;
    try {
      apply(
        await saveSettings(snapshot.value.summonShortcut, snapshot.value.launchAtStartup, value)
      );
    } catch (error) {
      formError.value = errorMessage(error);
    } finally {
      busy.value = false;
    }
  }

  function onShortcutKey(event: KeyboardEvent) {
    if (!listening.value || event.isComposing) return;
    event.preventDefault();
    event.stopPropagation();
    if (event.key === 'Escape') {
      listening.value = false;
      return;
    }
    const shortcut = captureShortcut(event);
    if (!shortcut) {
      formError.value = 'Use at least one modifier and a letter, number, or space.';
      return;
    }
    listening.value = false;
    void commitShortcut(shortcut);
  }

  function captureShortcut(event: KeyboardEvent): string | null {
    const parts: string[] = [];
    if (event.ctrlKey) parts.push('ctrl');
    if (event.altKey) parts.push('alt');
    if (event.shiftKey) parts.push('shift');
    if (event.metaKey) parts.push('super');
    if (parts.length === 0) return null;
    if (['Control', 'Alt', 'Shift', 'Meta'].includes(event.key)) return null;
    if (event.key === ' ' || event.key === 'Spacebar') {
      parts.push('space');
    } else if (event.key.length === 1) {
      parts.push(event.key.toLowerCase());
    } else {
      return null;
    }
    return parts.join('+');
  }

  async function commitShortcut(shortcut: string) {
    if (!snapshot.value) return;
    busy.value = true;
    try {
      apply(
        await saveSettings(
          shortcut,
          snapshot.value.launchAtStartup,
          snapshot.value.defaultDestinationId
        )
      );
      status.value = `Summon shortcut is ${formatShortcut(shortcut)}.`;
    } catch (error) {
      formError.value = errorMessage(error);
    } finally {
      busy.value = false;
    }
  }

  function startEdit(destination: Destination) {
    editingId.value = destination.id;
    draft.value = {
      name: destination.name,
      triggersText: destination.triggers.join(', '),
      urlTemplate: destination.urlTemplate,
      suggest: destination.suggest,
    };
    formError.value = null;
  }

  async function saveDraft(destination: Destination) {
    busy.value = true;
    try {
      apply(
        await saveDestination({
          ...destination,
          name: draft.value.name,
          triggers: splitTriggers(draft.value.triggersText),
          urlTemplate: draft.value.urlTemplate,
          suggest: draft.value.suggest,
        })
      );
      editingId.value = null;
    } catch (error) {
      formError.value = errorMessage(error);
    } finally {
      busy.value = false;
    }
  }

  async function patch(destination: Destination, partial: Partial<Destination>) {
    busy.value = true;
    try {
      apply(await saveDestination({ ...destination, ...partial }));
    } catch (error) {
      formError.value = errorMessage(error);
    } finally {
      busy.value = false;
    }
  }

  async function move(id: string, delta: number) {
    busy.value = true;
    try {
      apply(await moveDestination(id, delta));
    } catch (error) {
      formError.value = errorMessage(error);
    } finally {
      busy.value = false;
    }
  }

  async function remove(id: string) {
    busy.value = true;
    try {
      apply(await removeDestination(id));
    } catch (error) {
      formError.value = errorMessage(error);
    } finally {
      busy.value = false;
    }
  }

  async function addDestination() {
    busy.value = true;
    try {
      apply(
        await saveDestination({
          id: `custom-${crypto.randomUUID()}`,
          name: added.value.name,
          triggers: splitTriggers(added.value.triggersText),
          urlTemplate: added.value.urlTemplate,
          suggest: added.value.suggest,
          pinned: true,
          builtin: false,
          disabled: false,
        })
      );
      added.value = {
        name: '',
        triggersText: '',
        urlTemplate: 'https://example.com/search?q={query}',
        suggest: 'none',
      };
      status.value = 'Destination added.';
    } catch (error) {
      formError.value = errorMessage(error);
    } finally {
      busy.value = false;
    }
  }

  function splitTriggers(value: string): string[] {
    return value
      .split(',')
      .map((trigger) => trigger.trim())
      .filter((trigger) => trigger.length > 0);
  }

  async function wipeHistory() {
    busy.value = true;
    try {
      apply(await clearHistory());
      status.value = 'History cleared.';
    } catch (error) {
      formError.value = errorMessage(error);
    } finally {
      busy.value = false;
    }
  }

  async function updates() {
    updateMessage.value = 'Checking…';
    try {
      updateMessage.value = await checkForUpdates();
    } catch (error) {
      updateMessage.value = errorMessage(error);
    }
  }

  function destinationName(id: string): string {
    if (id === 'url') return 'Link';
    return snapshot.value?.destinations.find((destination) => destination.id === id)?.name ?? id;
  }
</script>

<template>
  <main class="settings">
    <header class="settings-header">
      <div>
        <h1>Zephyr</h1>
        <p>Summon the bar, type once, and send it somewhere.</p>
      </div>
      <button type="button" class="primary" @click="showBar">Show Zephyr</button>
    </header>

    <p v-if="formError" class="notice">{{ formError }}</p>
    <p v-else-if="status" class="status">{{ status }}</p>

    <section v-if="snapshot" id="section-general">
      <h2>General</h2>
      <div class="field">
        <span>Summon shortcut</span>
        <button type="button" class="recorder" :class="{ listening }" @click="listening = true">
          {{ listening ? 'Press a shortcut' : formatShortcut(snapshot.summonShortcut) }}
        </button>
      </div>
      <label class="check">
        <input
          type="checkbox"
          :checked="snapshot.launchAtStartup"
          :disabled="busy"
          @change="toggleStartup"
        />
        Launch at startup
      </label>
      <label class="field">
        <span>Default destination</span>
        <select :value="snapshot.defaultDestinationId" :disabled="busy" @change="changeDefault">
          <option
            v-for="destination in snapshot.destinations.filter((item) => !item.disabled)"
            :key="destination.id"
            :value="destination.id"
          >
            {{ destination.name }}
          </option>
        </select>
      </label>
      <div class="field">
        <button type="button" @click="updates">Check for updates</button>
        <span v-if="updateMessage" class="hint-text">{{ updateMessage }}</span>
      </div>
    </section>

    <section v-if="snapshot" id="section-destinations">
      <h2>Destinations</h2>
      <p class="lede">
        The first eight pinned destinations get Ctrl+1 through Ctrl+8. A trigger is what you type
        after !.
      </p>
      <article
        v-for="(destination, index) in snapshot.destinations"
        :key="destination.id"
        class="destination"
      >
        <div class="destination-row">
          <div>
            <strong>{{ destination.name }}</strong>
            <span class="hint-text">{{
              destination.triggers.map((trigger) => `!${trigger}`).join(' ')
            }}</span>
            <span v-if="destination.builtin" class="badge">Built-in</span>
          </div>
          <div class="row-actions">
            <button type="button" :disabled="busy || index === 0" @click="move(destination.id, -1)">
              Up
            </button>
            <button
              type="button"
              :disabled="busy || index === snapshot.destinations.length - 1"
              @click="move(destination.id, 1)"
            >
              Down
            </button>
            <button
              type="button"
              :disabled="busy"
              @click="patch(destination, { pinned: !destination.pinned })"
            >
              {{ destination.pinned ? 'Unpin' : 'Pin' }}
            </button>
            <button
              type="button"
              :disabled="busy"
              @click="patch(destination, { disabled: !destination.disabled })"
            >
              {{ destination.disabled ? 'Turn on' : 'Turn off' }}
            </button>
            <button type="button" @click="startEdit(destination)">Edit</button>
            <button
              v-if="!destination.builtin"
              type="button"
              :disabled="busy"
              @click="remove(destination.id)"
            >
              Delete
            </button>
          </div>
        </div>

        <form
          v-if="editingId === destination.id"
          class="editor"
          @submit.prevent="saveDraft(destination)"
        >
          <label class="field">
            <span>Name</span>
            <input v-model="draft.name" type="text" maxlength="40" required />
          </label>
          <label class="field">
            <span>Triggers</span>
            <input v-model="draft.triggersText" type="text" placeholder="wiki, w" required />
          </label>
          <label class="field">
            <span>URL template</span>
            <input v-model="draft.urlTemplate" type="url" required />
          </label>
          <label class="field">
            <span>Suggestions</span>
            <select v-model="draft.suggest">
              <option value="none">None</option>
              <option value="google">Google</option>
              <option value="youtube">YouTube</option>
              <option value="wikipedia">Wikipedia</option>
              <option value="pubmed">PubMed</option>
            </select>
          </label>
          <div class="row-actions">
            <button class="primary" type="submit" :disabled="busy">Save</button>
            <button type="button" @click="editingId = null">Cancel</button>
          </div>
        </form>
      </article>

      <form class="editor add" @submit.prevent="addDestination">
        <h3>Add a destination</h3>
        <label class="field">
          <span>Name</span>
          <input v-model="added.name" type="text" maxlength="40" required />
        </label>
        <label class="field">
          <span>Triggers</span>
          <input v-model="added.triggersText" type="text" placeholder="arxiv" required />
        </label>
        <label class="field">
          <span>URL template</span>
          <input v-model="added.urlTemplate" type="url" required />
        </label>
        <label class="field">
          <span>Suggestions</span>
          <select v-model="added.suggest">
            <option value="none">None</option>
            <option value="google">Google</option>
            <option value="youtube">YouTube</option>
            <option value="wikipedia">Wikipedia</option>
            <option value="pubmed">PubMed</option>
          </select>
        </label>
        <button class="primary" type="submit" :disabled="busy">Add destination</button>
      </form>
    </section>

    <section v-if="snapshot" id="section-history">
      <h2>History</h2>
      <ul v-if="historyPreview.length" class="history">
        <li v-for="entry in historyPreview" :key="`${entry.destinationId}-${entry.query}`">
          <span>{{ entry.query }}</span>
          <span class="hint-text"
            >{{ destinationName(entry.destinationId) }} · {{ entry.uses }}</span
          >
        </li>
      </ul>
      <p v-else class="lede">No searches yet.</p>
      <button type="button" :disabled="busy || snapshot.history.length === 0" @click="wipeHistory">
        Clear history
      </button>
    </section>
  </main>
</template>
