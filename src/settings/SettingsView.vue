<script setup lang="ts">
  import { computed, nextTick, onMounted, onUnmounted, ref } from 'vue';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import {
    aiDetectLocal,
    aiHasKey,
    aiModels,
    aiPresets,
    aiSetKey,
    checkForUpdates,
    clearHistory,
    errorMessage,
    fileIndexStatus,
    formatShortcut,
    getSnapshot,
    moveDestination,
    rebuildFileIndex,
    removeDestination,
    saveDestination,
    saveAiSettings,
    saveFileFolders,
    saveSettings,
    showBar,
  } from '../api';
  import {
    isSnapshot,
    type AiPreset,
    type AiSettings,
    type Destination,
    type FileIndexStatus,
    type LocalServer,
    type Snapshot,
    type SuggestKind,
  } from '../types';

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

  const fileStatus = ref<FileIndexStatus | null>(null);
  const newRoot = ref('');
  const newExclude = ref('');

  const presets = ref<AiPreset[]>([]);
  const aiDraft = ref<AiSettings>({ provider: 'auto', model: '', baseUrl: '', api: 'openai' });
  const aiKey = ref('');
  const aiKeySaved = ref(false);
  const aiModelList = ref<string[]>([]);
  const aiLocal = ref<LocalServer[] | null>(null);
  const aiMessage = ref<string | null>(null);

  let unlistens: UnlistenFn[] = [];
  let statusTimer = 0;

  const aiPreset = computed(() =>
    presets.value.find((preset) => preset.id === aiDraft.value.provider)
  );
  const aiNeedsKey = computed(
    () => aiDraft.value.provider === 'custom' || Boolean(aiPreset.value?.needsKey)
  );
  const aiProviderName = computed(() => aiPreset.value?.name ?? 'Server');
  const aiShowsLocal = computed(
    () => aiDraft.value.provider === 'auto' || Boolean(aiPreset.value?.local)
  );

  const localSummary = computed(() => {
    const servers = aiLocal.value;
    if (servers === null) return 'Looking for Ollama and LM Studio…';
    if (servers.length === 0) {
      return 'No local model server is running. Start Ollama or LM Studio, or pick a provider and add your key.';
    }
    return servers
      .map((server) => `${server.name} is running with ${server.models.length} models`)
      .join('. ');
  });

  const modelPlaceholder = computed(() => {
    const preset = aiPreset.value;
    if (preset?.defaultModel) return `Default: ${preset.defaultModel}`;
    if (preset?.local) return 'Default: the first loaded model';
    return 'Model id, or use Load models';
  });

  // Saved roots, or the default (home) the index reports when none are saved.
  const fileRoots = computed(() => snapshot.value?.fileRoots ?? fileStatus.value?.roots ?? []);

  const indexSummary = computed(() => {
    const current = fileStatus.value;
    if (!current) return '';
    if (current.roots.length === 0) return 'File search is off. Add a folder to turn it on.';
    const count = current.entries.toLocaleString();
    return current.scanning ? `Indexing… ${count} items so far` : `${count} items indexed`;
  });

  const historyPreview = computed(() => snapshot.value?.history.slice(0, 8) ?? []);

  onMounted(async () => {
    await refresh();
    void loadAi();
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
    void pollFileStatus();
  });

  onUnmounted(() => {
    window.clearTimeout(statusTimer);
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

  // Poll while a walk runs so the count climbs, then stop.
  async function pollFileStatus() {
    window.clearTimeout(statusTimer);
    try {
      fileStatus.value = await fileIndexStatus();
    } catch {
      return;
    }
    if (fileStatus.value.scanning) {
      statusTimer = window.setTimeout(() => void pollFileStatus(), 1000);
    }
  }

  async function saveFolders(roots: string[] | null, excludes: string[]) {
    busy.value = true;
    try {
      apply(await saveFileFolders(roots, excludes));
      // The new walk starts on a worker thread; give it a moment to report.
      window.setTimeout(() => void pollFileStatus(), 300);
      return true;
    } catch (error) {
      formError.value = errorMessage(error);
      return false;
    } finally {
      busy.value = false;
    }
  }

  async function addRoot() {
    if (!snapshot.value || !newRoot.value.trim()) return;
    if (await saveFolders([...fileRoots.value, newRoot.value], snapshot.value.fileExcludes)) {
      newRoot.value = '';
    }
  }

  function removeRoot(root: string) {
    if (!snapshot.value) return;
    void saveFolders(
      fileRoots.value.filter((item) => item !== root),
      snapshot.value.fileExcludes
    );
  }

  function useHomeFolder() {
    if (!snapshot.value) return;
    void saveFolders(null, snapshot.value.fileExcludes);
  }

  async function addExclude() {
    if (!snapshot.value || !newExclude.value.trim()) return;
    if (
      await saveFolders(snapshot.value.fileRoots, [
        ...snapshot.value.fileExcludes,
        newExclude.value,
      ])
    ) {
      newExclude.value = '';
    }
  }

  function removeExclude(exclude: string) {
    if (!snapshot.value) return;
    void saveFolders(
      snapshot.value.fileRoots,
      snapshot.value.fileExcludes.filter((item) => item !== exclude)
    );
  }

  async function rebuild() {
    await rebuildFileIndex();
    window.setTimeout(() => void pollFileStatus(), 300);
  }

  async function loadAi() {
    if (snapshot.value) aiDraft.value = { ...snapshot.value.ai };
    try {
      presets.value = await aiPresets();
    } catch (error) {
      formError.value = errorMessage(error);
    }
    void refreshKeyState();
    void detectLocal();
  }

  async function detectLocal() {
    aiLocal.value = null;
    try {
      aiLocal.value = await aiDetectLocal();
    } catch {
      aiLocal.value = [];
    }
  }

  async function refreshKeyState() {
    aiKeySaved.value = aiNeedsKey.value ? await aiHasKey(aiDraft.value.provider) : false;
  }

  async function saveAi() {
    busy.value = true;
    try {
      apply(await saveAiSettings(aiDraft.value));
      aiDraft.value = { ...(snapshot.value?.ai ?? aiDraft.value) };
      aiMessage.value = 'Saved.';
    } catch (error) {
      formError.value = errorMessage(error);
    } finally {
      busy.value = false;
    }
  }

  async function changeProvider(event: Event) {
    const provider = (event.target as HTMLSelectElement).value;
    aiDraft.value = { ...aiDraft.value, provider, model: '' };
    aiModelList.value = [];
    aiKey.value = '';
    aiMessage.value = null;
    // A custom server needs its URL before it can be saved.
    if (provider !== 'custom' || aiDraft.value.baseUrl) await saveAi();
    await refreshKeyState();
  }

  async function saveKey() {
    busy.value = true;
    try {
      await aiSetKey(aiDraft.value.provider, aiKey.value);
      aiKey.value = '';
      aiKeySaved.value = true;
      aiMessage.value = 'Key saved in your keychain.';
    } catch (error) {
      formError.value = errorMessage(error);
    } finally {
      busy.value = false;
    }
  }

  async function removeKey() {
    busy.value = true;
    try {
      await aiSetKey(aiDraft.value.provider, null);
      aiKeySaved.value = false;
      aiMessage.value = 'Key removed.';
    } catch (error) {
      formError.value = errorMessage(error);
    } finally {
      busy.value = false;
    }
  }

  async function loadModels() {
    aiMessage.value = 'Loading models…';
    try {
      aiModelList.value = await aiModels(aiDraft.value);
      aiMessage.value = aiModelList.value.length
        ? `${aiModelList.value.length} models. Pick one in the model box.`
        : 'No models found.';
    } catch (error) {
      aiMessage.value = errorMessage(error);
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

    <section v-if="snapshot" id="section-ai">
      <h2>AI</h2>
      <p class="lede">
        Press Tab to arm Ask AI, or type !ai, and the answer streams into the bar. Enter copies it.
        Requests go straight from Zephyr to the provider you pick.
      </p>
      <label class="field">
        <span>Provider</span>
        <select :value="aiDraft.provider" :disabled="busy" @change="changeProvider">
          <option value="auto">Automatic: a local model if one is running</option>
          <option v-for="preset in presets" :key="preset.id" :value="preset.id">
            {{ preset.name }}{{ preset.local ? ' (local)' : '' }}
          </option>
          <option value="custom">Custom server</option>
        </select>
      </label>
      <div v-if="aiShowsLocal" class="field inline">
        <span class="hint-text">{{ localSummary }}</span>
        <button type="button" @click="detectLocal">Check again</button>
      </div>
      <template v-if="aiDraft.provider === 'custom'">
        <label class="field">
          <span>Base URL</span>
          <input
            v-model="aiDraft.baseUrl"
            type="url"
            placeholder="http://localhost:8080/v1"
            @change="saveAi"
          />
        </label>
        <label class="field">
          <span>API format</span>
          <select v-model="aiDraft.api" @change="saveAi">
            <option value="openai">OpenAI-compatible</option>
            <option value="anthropic">Anthropic</option>
          </select>
        </label>
      </template>
      <template v-if="aiNeedsKey">
        <h3>API key</h3>
        <form class="field inline" @submit.prevent="saveKey">
          <input
            v-model="aiKey"
            type="password"
            autocomplete="off"
            :placeholder="aiKeySaved ? 'Saved in your keychain' : `${aiProviderName} API key`"
          />
          <button type="submit" :disabled="busy || !aiKey.trim()">Save key</button>
          <button v-if="aiKeySaved" type="button" :disabled="busy" @click="removeKey">
            Remove key
          </button>
        </form>
      </template>
      <template v-if="aiDraft.provider !== 'auto'">
        <h3>Model</h3>
        <form class="field inline" @submit.prevent="saveAi">
          <input
            v-model="aiDraft.model"
            type="text"
            list="ai-models"
            spellcheck="false"
            :placeholder="modelPlaceholder"
          />
          <datalist id="ai-models">
            <option v-for="model in aiModelList" :key="model" :value="model" />
          </datalist>
          <button type="button" :disabled="busy" @click="loadModels">Load models</button>
          <button type="submit" :disabled="busy">Save model</button>
        </form>
      </template>
      <p v-if="aiMessage" class="hint-text">{{ aiMessage }}</p>
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
          <label v-if="destination.kind !== 'ai'" class="field">
            <span>URL template</span>
            <input v-model="draft.urlTemplate" type="url" required />
          </label>
          <label v-if="destination.kind !== 'ai'" class="field">
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

    <section v-if="snapshot" id="section-files">
      <h2>Files</h2>
      <p class="lede">
        Type !f and a name to open a file or folder; Ctrl+Enter shows it in its folder. Hidden
        folders, .gitignored files and caches like node_modules are skipped.
      </p>
      <h3>Folders to search</h3>
      <ul class="folders">
        <li v-for="root in fileRoots" :key="root">
          <span class="path">{{ root }}</span>
          <button type="button" :disabled="busy" @click="removeRoot(root)">Remove</button>
        </li>
      </ul>
      <form class="field inline" @submit.prevent="addRoot">
        <input
          v-model="newRoot"
          type="text"
          placeholder="Folder path, e.g. D:\Projects or ~/Work"
        />
        <button type="submit" :disabled="busy || !newRoot.trim()">Add folder</button>
        <button
          v-if="snapshot.fileRoots !== null"
          type="button"
          :disabled="busy"
          @click="useHomeFolder"
        >
          Use home folder
        </button>
      </form>
      <h3>Folders to skip</h3>
      <ul v-if="snapshot.fileExcludes.length" class="folders">
        <li v-for="exclude in snapshot.fileExcludes" :key="exclude">
          <span class="path">{{ exclude }}</span>
          <button type="button" :disabled="busy" @click="removeExclude(exclude)">Remove</button>
        </li>
      </ul>
      <form class="field inline" @submit.prevent="addExclude">
        <input v-model="newExclude" type="text" placeholder="Folder path to leave out" />
        <button type="submit" :disabled="busy || !newExclude.trim()">Skip folder</button>
      </form>
      <div class="field inline">
        <span class="hint-text">{{ indexSummary }}</span>
        <button type="button" :disabled="fileStatus?.scanning" @click="rebuild">
          Rebuild index
        </button>
      </div>
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
