<script setup lang="ts">
  import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import {
    aiDetectLocal,
    aiHasKey,
    aiModels,
    aiPresets,
    aiSetKey,
    checkForUpdates,
    claudeStatus,
    saveClaudeSettings,
    clipCanPaste,
    clipClear,
    clipRequestPastePermission,
    clipStats,
    openNotes,
    pageReady,
    pickFolder,
    revealNotes,
    saveClipboardSettings,
    saveOpenBehavior,
    saveResumeSeconds,
    saveShellProgram,
    saveShellTerminal,
    clearShellHistory,
    shellInfo,
    saveNotesShortcut,
    clearHistory,
    errorMessage,
    exportDestinations,
    fileIndexStatus,
    importDestinations,
    formatShortcut,
    getSnapshot,
    moveDestination,
    rebuildFileIndex,
    removeDestination,
    saveDestination,
    saveAiSettings,
    saveFileFolders,
    saveSettings,
  } from '../api';
  import SyncSection from './SyncSection.vue';
  import SelectMenu from './SelectMenu.vue';
  import { GLYPHS } from '../bar/glyphs';
  import {
    isSnapshot,
    type AiPreset,
    type AiSettings,
    type ClaudeCliStatus,
    type ClaudeProject,
    type ClaudeSettings,
    type ClipboardSettings,
    type Destination,
    type FileIndexStatus,
    type LocalServer,
    type ShellInfo,
    type ShellProgram,
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
    suggestUrl: '',
    suggestPath: '',
  });

  const added = ref({
    name: '',
    triggersText: '',
    urlTemplate: 'https://example.com/search?q={query}',
    suggest: 'none' as SuggestKind,
    suggestUrl: '',
    suggestPath: '',
  });

  const importText = ref('');
  const importOpen = ref(false);

  const fileStatus = ref<FileIndexStatus | null>(null);
  const newRoot = ref('');
  const newExclude = ref('');

  const notesListening = ref(false);
  const cli = ref<ClaudeCliStatus | null>(null);
  const newProjectFolder = ref('');
  const clipListening = ref(false);
  const clipNewApp = ref('');
  const clipCount = ref<number | null>(null);
  const clipPasteAllowed = ref<boolean | null>(null);
  const clipConfirm = ref(false);
  const RETENTION = [
    { days: 7, label: '7 days' },
    { days: 30, label: '1 month' },
    { days: 90, label: '3 months' },
    { days: 180, label: '6 months' },
    { days: 365, label: '1 year' },
    { days: 0, label: 'Until the 2,000 item limit' },
  ];

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
    pageReady();
    void loadAi();
    void loadClip();
    void loadCli();
    void loadShell();
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

  // Sidebar sections; one shows at a time.
  const SECTIONS = [
    { id: 'general', label: 'General', icon: 'gear', hue: 250 },
    { id: 'ai', label: 'AI', icon: 'sparkle', hue: 275 },
    { id: 'claude', label: 'Claude', icon: 'terminal', hue: 20 },
    { id: 'shell', label: 'Shell', icon: 'prompt', hue: 130 },
    { id: 'sync', label: 'Sync', icon: 'sync', hue: 200 },
    { id: 'clipboard', label: 'Clipboard', icon: 'clipboard', hue: 160 },
    { id: 'notes', label: 'Notes', icon: 'note', hue: 45 },
    { id: 'destinations', label: 'Destinations', icon: 'link', hue: 300 },
    { id: 'files', label: 'Files', icon: 'folder', hue: 215 },
    { id: 'history', label: 'History', icon: 'clock', hue: 340 },
  ];
  const active = ref('general');
  const search = ref('');
  const searchEl = ref<HTMLInputElement | null>(null);
  const paneEl = ref<HTMLElement | null>(null);
  const settingsEl = ref<HTMLElement | null>(null);
  const resultIndex = ref(0);

  interface Result {
    section: string;
    label: string;
    element: HTMLElement;
  }
  const results = ref<Result[]>([]);

  function sectionLabel(id: string): string {
    return SECTIONS.find((section) => section.id === id)?.label ?? id;
  }

  function open(id: string) {
    search.value = '';
    active.value = id;
    paneEl.value?.scrollTo({ top: 0 });
  }

  /**
   * Search reads the labels already on the page, so every setting is findable without a
   * separate index to keep in step.
   */
  watch(search, async () => {
    resultIndex.value = 0;
    const query = search.value.trim().toLowerCase();
    if (!query) {
      results.value = [];
      return;
    }
    await nextTick();
    const found: Result[] = [];
    const seen = new Set<string>();
    for (const section of SECTIONS) {
      const root = document.getElementById(`section-${section.id}`);
      if (!root) continue;
      if (section.label.toLowerCase().includes(query)) {
        found.push({ section: section.id, label: section.label, element: root });
      }
      const labels = root.querySelectorAll<HTMLElement>(
        '.field > span:first-child, label.check, h3, .destination strong, button'
      );
      for (const element of labels) {
        const text = (element.textContent ?? '').replace(/\s+/g, ' ').trim();
        const key = `${section.id}:${text}`;
        if (!text || text.length > 80 || seen.has(key) || !text.toLowerCase().includes(query)) {
          continue;
        }
        seen.add(key);
        found.push({ section: section.id, label: text, element });
      }
    }
    results.value = found.slice(0, 30);
  });

  async function jump(result: Result) {
    search.value = '';
    active.value = result.section;
    await nextTick();
    const target =
      (result.element.closest('.field, .check, .destination, h3') as HTMLElement | null) ??
      result.element;
    const still = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    target.scrollIntoView({ block: 'center', behavior: still ? 'auto' : 'smooth' });
    target.classList.remove('revealed');
    void target.offsetWidth;
    target.classList.add('revealed');
  }

  function onSearchKey(event: KeyboardEvent) {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      const count = results.value.length;
      if (count) {
        resultIndex.value =
          (resultIndex.value + (event.key === 'ArrowDown' ? 1 : -1) + count) % count;
      }
    } else if (event.key === 'Enter') {
      event.preventDefault();
      const result = results.value[resultIndex.value];
      if (result) void jump(result);
    } else if (event.key === 'Escape' && search.value) {
      event.preventDefault();
      search.value = '';
    }
  }

  /** The bar's !set opens a section directly. */
  async function revealSection(section: string) {
    if (SECTIONS.some((item) => item.id === section)) open(section);
    await nextTick();
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
    // ⌘F / Ctrl+F jumps to the settings search.
    if ((event.metaKey || event.ctrlKey) && event.code === 'KeyF') {
      event.preventDefault();
      searchEl.value?.focus();
      searchEl.value?.select();
      return;
    }
    if (notesListening.value && !event.isComposing) {
      event.preventDefault();
      event.stopPropagation();
      notesListening.value = false;
      if (event.key === 'Escape') return;
      const shortcut = event.key === 'Backspace' ? '' : captureShortcut(event);
      if (shortcut === null) {
        formError.value = 'Use at least one modifier and a letter, number, or space.';
        return;
      }
      void saveNotesKey(shortcut);
      return;
    }
    if (clipListening.value && !event.isComposing) {
      event.preventDefault();
      event.stopPropagation();
      clipListening.value = false;
      if (event.key === 'Escape') return;
      const shortcut = captureShortcut(event);
      if (!shortcut) {
        formError.value = 'Use at least one modifier and a letter, number, or space.';
        return;
      }
      void saveClip({ shortcut });
      return;
    }
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
      suggestUrl: destination.suggestUrl ?? '',
      suggestPath: destination.suggestPath ?? '',
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
          suggestUrl: draft.value.suggestUrl,
          suggestPath: draft.value.suggestPath,
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
          suggestUrl: added.value.suggestUrl,
          suggestPath: added.value.suggestPath,
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
        suggestUrl: '',
        suggestPath: '',
      };
      status.value = 'Destination added.';
    } catch (error) {
      formError.value = errorMessage(error);
    } finally {
      busy.value = false;
    }
  }

  async function copyDestinations() {
    try {
      await navigator.clipboard.writeText(await exportDestinations());
      status.value = 'Copied your destinations as JSON.';
    } catch (error) {
      formError.value = errorMessage(error);
    }
  }

  async function runImport() {
    busy.value = true;
    try {
      const before = snapshot.value?.destinations.length ?? 0;
      apply(await importDestinations(importText.value));
      const added = (snapshot.value?.destinations.length ?? 0) - before;
      status.value = added > 0 ? `Imported. ${added} new destinations.` : 'Imported.';
      importText.value = '';
      importOpen.value = false;
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

  const isMacOs = navigator.userAgent.includes('Mac');
  const RESUME = [
    { seconds: 0, label: 'Nothing: start empty' },
    { seconds: 30, label: '30 seconds' },
    { seconds: 120, label: '2 minutes' },
    { seconds: 600, label: '10 minutes' },
    { seconds: 3600, label: 'An hour' },
    { seconds: 86400, label: 'Always' },
  ];

  async function saveBehavior(partial: {
    returnToMode?: boolean;
    routeCmd?: string;
    routeAlt?: string;
  }) {
    if (!snapshot.value) return;
    busy.value = true;
    try {
      apply(
        await saveOpenBehavior(
          partial.returnToMode ?? snapshot.value.returnToMode,
          partial.routeCmd ?? snapshot.value.routeCmd,
          partial.routeAlt ?? snapshot.value.routeAlt
        )
      );
    } catch (error) {
      formError.value = errorMessage(error);
    } finally {
      busy.value = false;
    }
  }

  const routeTargets = computed(() => {
    const targets: { value: string; label: string }[] = [];
    for (const destination of snapshot.value?.destinations ?? []) {
      if (!destination.disabled) targets.push({ value: destination.id, label: destination.name });
    }
    for (const project of snapshot.value?.claude.projects ?? []) {
      const name = project.alias || project.folder.split(/[\\/]/).pop();
      targets.push({ value: `claude:${project.id}`, label: `Claude job in ${name}` });
    }
    return targets;
  });

  async function changeResume(event: Event) {
    busy.value = true;
    try {
      apply(await saveResumeSeconds(Number((event.target as HTMLSelectElement).value)));
    } catch (error) {
      formError.value = errorMessage(error);
    } finally {
      busy.value = false;
    }
  }

  const shell = ref<ShellInfo | null>(null);
  const SHELLS: { id: ShellProgram; label: string }[] = [
    { id: 'gitbash', label: 'Git Bash' },
    { id: 'powershell', label: 'PowerShell' },
    { id: 'wsl', label: 'WSL (bash in Linux)' },
  ];

  async function loadShell() {
    try {
      shell.value = await shellInfo();
    } catch {
      shell.value = null;
    }
  }

  async function changeShell(event: Event) {
    busy.value = true;
    try {
      apply(await saveShellProgram((event.target as HTMLSelectElement).value as ShellProgram));
      await loadShell();
    } catch (error) {
      formError.value = errorMessage(error);
    } finally {
      busy.value = false;
    }
  }

  async function changeTerminal(event: Event) {
    busy.value = true;
    try {
      const value = (event.target as HTMLSelectElement).value as 'auto' | 'console';
      apply(await saveShellTerminal(value));
      await loadShell();
    } catch (error) {
      formError.value = errorMessage(error);
    } finally {
      busy.value = false;
    }
  }

  async function forgetCommands() {
    busy.value = true;
    try {
      apply(await clearShellHistory());
    } catch (error) {
      formError.value = errorMessage(error);
    } finally {
      busy.value = false;
    }
  }

  async function loadCli() {
    try {
      cli.value = await claudeStatus();
    } catch {
      cli.value = null;
    }
  }

  async function saveClaude(next: ClaudeSettings) {
    busy.value = true;
    try {
      apply(await saveClaudeSettings(next));
      return true;
    } catch (error) {
      formError.value = errorMessage(error);
      return false;
    } finally {
      busy.value = false;
    }
  }

  async function addProject() {
    if (!snapshot.value || !newProjectFolder.value.trim()) return;
    const claude = snapshot.value.claude;
    const project: ClaudeProject = {
      id: '',
      folder: newProjectFolder.value.trim(),
      alias: '',
      profile: 'edit',
      allow: [],
      model: '',
      effort: '',
      note: '',
    };
    if (await saveClaude({ ...claude, projects: [...claude.projects, project] })) {
      newProjectFolder.value = '';
      status.value =
        'Project added. Claude runs that folder’s own .claude hooks and .mcp.json servers without asking.';
    }
  }

  async function chooseProjectFolder() {
    const folder = await pickFolder().catch(() => null);
    if (!folder) return;
    newProjectFolder.value = folder;
    await addProject();
  }

  async function chooseRoot() {
    const folder = await pickFolder().catch(() => null);
    if (!folder || !snapshot.value) return;
    await saveFolders([...fileRoots.value, folder], snapshot.value.fileExcludes);
  }

  function updateProject(id: string, partial: Partial<ClaudeProject>) {
    if (!snapshot.value) return;
    const claude = snapshot.value.claude;
    void saveClaude({
      ...claude,
      projects: claude.projects.map((project) =>
        project.id === id ? { ...project, ...partial } : project
      ),
    });
  }

  function removeProject(id: string) {
    if (!snapshot.value) return;
    const claude = snapshot.value.claude;
    void saveClaude({
      ...claude,
      projects: claude.projects.filter((project) => project.id !== id),
    });
  }

  function saveClaudeLimits(partial: Partial<ClaudeSettings>) {
    if (!snapshot.value) return;
    void saveClaude({ ...snapshot.value.claude, ...partial });
  }

  async function saveNotesKey(shortcut: string) {
    busy.value = true;
    try {
      apply(await saveNotesShortcut(shortcut));
    } catch (error) {
      formError.value = errorMessage(error);
    } finally {
      busy.value = false;
    }
  }

  async function loadClip() {
    try {
      clipCount.value = (await clipStats()).total ?? 0;
      clipPasteAllowed.value = await clipCanPaste();
    } catch {
      clipCount.value = null;
    }
  }

  async function saveClip(partial: Partial<ClipboardSettings>) {
    if (!snapshot.value) return;
    busy.value = true;
    try {
      apply(await saveClipboardSettings({ ...snapshot.value.clipboard, ...partial }));
      void loadClip();
    } catch (error) {
      formError.value = errorMessage(error);
    } finally {
      busy.value = false;
    }
  }

  function addIgnoredApp() {
    const app = clipNewApp.value.trim();
    if (!snapshot.value || !app) return;
    void saveClip({ ignoredApps: [...snapshot.value.clipboard.ignoredApps, app] });
    clipNewApp.value = '';
  }

  function removeIgnoredApp(app: string) {
    if (!snapshot.value) return;
    void saveClip({
      ignoredApps: snapshot.value.clipboard.ignoredApps.filter((item) => item !== app),
    });
  }

  async function clearClipboard() {
    if (!clipConfirm.value) {
      clipConfirm.value = true;
      return;
    }
    clipConfirm.value = false;
    try {
      const removed = await clipClear(false);
      status.value = `Deleted ${removed} clipboard items. Pinned items stayed.`;
      void loadClip();
    } catch (error) {
      formError.value = errorMessage(error);
    }
  }

  async function allowPasting() {
    clipPasteAllowed.value = await clipRequestPastePermission();
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
  <main ref="settingsEl" class="settings">
    <aside class="settings-nav">
      <input
        ref="searchEl"
        v-model="search"
        class="settings-search"
        type="search"
        placeholder="Search settings"
        spellcheck="false"
        @keydown="onSearchKey"
      />
      <nav aria-label="Settings sections">
        <button
          v-for="section in SECTIONS"
          :key="section.id"
          type="button"
          class="nav-item"
          :class="{ active: !search && active === section.id }"
          @click="open(section.id)"
        >
          <span class="nav-icon" :style="{ '--hue': section.hue }" aria-hidden="true">
            <svg
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="1.9"
              stroke-linecap="round"
              stroke-linejoin="round"
              v-html="GLYPHS[section.icon]"
            />
          </span>
          {{ section.label }}
        </button>
      </nav>
    </aside>

    <div ref="paneEl" class="settings-pane">
      <p v-if="formError" class="notice">{{ formError }}</p>
      <p v-else-if="status" class="status">{{ status }}</p>

      <section v-if="search" class="search-results">
        <h2>Results</h2>
        <ul v-if="results.length" class="results-list">
          <li v-for="(result, index) in results" :key="index">
            <button
              type="button"
              :class="{ selected: index === resultIndex }"
              @click="jump(result)"
              @mousemove="resultIndex = index"
            >
              <span>{{ result.label }}</span>
              <span class="hint-text">{{ sectionLabel(result.section) }}</span>
            </button>
          </li>
        </ul>
        <p v-else class="hint-text">No settings match “{{ search }}”.</p>
      </section>

      <section v-if="snapshot" v-show="!search && active === 'general'" id="section-general">
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
        <label class="field">
          <span>When Zephyr opens</span>
          <select
            :value="snapshot.returnToMode ? 'return' : 'search'"
            :disabled="busy"
            @change="
              saveBehavior({
                returnToMode: ($event.target as HTMLSelectElement).value === 'return',
              })
            "
          >
            <option value="return">Return to what I was doing</option>
            <option value="search">Always start at the search bar</option>
          </select>
        </label>
        <label class="field">
          <span>{{ isMacOs ? '⌘↵' : 'Ctrl+Enter' }} sends what you typed to</span>
          <select
            :value="snapshot.routeCmd"
            :disabled="busy"
            @change="saveBehavior({ routeCmd: ($event.target as HTMLSelectElement).value })"
          >
            <option value="">Ask AI (web search when AI is armed)</option>
            <option v-for="target in routeTargets" :key="target.value" :value="target.value">
              {{ target.label }}
            </option>
          </select>
        </label>
        <label class="field">
          <span>{{ isMacOs ? '⌥↵' : 'Alt+Enter' }} sends what you typed to</span>
          <select
            :value="snapshot.routeAlt"
            :disabled="busy"
            @change="saveBehavior({ routeAlt: ($event.target as HTMLSelectElement).value })"
          >
            <option value="">A Claude job in the last project used</option>
            <option v-for="target in routeTargets" :key="target.value" :value="target.value">
              {{ target.label }}
            </option>
          </select>
        </label>
        <label class="field">
          <span>Keep search text for</span>
          <select :value="snapshot.resumeSeconds" :disabled="busy" @change="changeResume">
            <option v-for="option in RESUME" :key="option.seconds" :value="option.seconds">
              {{ option.label }}
            </option>
          </select>
        </label>
        <div class="field">
          <button type="button" @click="updates">Check for updates</button>
          <span v-if="updateMessage" class="hint-text">{{ updateMessage }}</span>
        </div>
      </section>

      <section v-if="snapshot" v-show="!search && active === 'ai'" id="section-ai">
        <h2>AI</h2>
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

      <section v-if="snapshot" v-show="!search && active === 'clipboard'" id="section-clipboard">
        <h2>Clipboard</h2>
        <label class="check">
          <input
            type="checkbox"
            :checked="snapshot.clipboard.enabled"
            :disabled="busy"
            @change="saveClip({ enabled: !snapshot.clipboard.enabled })"
          />
          Keep clipboard history
        </label>
        <div class="field">
          <span>Shortcut</span>
          <button
            type="button"
            class="recorder"
            :class="{ listening: clipListening }"
            @click="clipListening = true"
          >
            {{
              clipListening
                ? 'Press a shortcut'
                : snapshot.clipboard.shortcut
                  ? formatShortcut(snapshot.clipboard.shortcut)
                  : 'None'
            }}
          </button>
        </div>
        <label class="field">
          <span>Keep items for</span>
          <select
            :value="snapshot.clipboard.retentionDays"
            :disabled="busy"
            @change="
              saveClip({ retentionDays: Number(($event.target as HTMLSelectElement).value) })
            "
          >
            <option v-for="option in RETENTION" :key="option.days" :value="option.days">
              {{ option.label }}
            </option>
          </select>
        </label>
        <label class="check">
          <input
            type="checkbox"
            :checked="snapshot.clipboard.recognizeText"
            :disabled="busy"
            @change="saveClip({ recognizeText: !snapshot.clipboard.recognizeText })"
          />
          Search text inside copied images
        </label>
        <div class="field inline">
          <span class="hint-text">
            {{
              clipPasteAllowed
                ? 'Zephyr can paste straight into the app you are using.'
                : 'To paste straight into apps, Zephyr needs Accessibility permission.'
            }}
          </span>
          <button v-if="clipPasteAllowed === false" type="button" @click="allowPasting">
            Allow pasting
          </button>
        </div>
        <h3>Never record from</h3>
        <ul class="folders">
          <li v-for="app in snapshot.clipboard.ignoredApps" :key="app">
            <span class="path">{{ app }}</span>
            <button type="button" :disabled="busy" @click="removeIgnoredApp(app)">Remove</button>
          </li>
        </ul>
        <form class="field inline" @submit.prevent="addIgnoredApp">
          <input v-model="clipNewApp" type="text" placeholder="App name or bundle id" />
          <button type="submit" :disabled="busy || !clipNewApp.trim()">Add app</button>
        </form>
        <div class="field inline">
          <span class="hint-text">{{
            clipCount === null ? '' : `${clipCount.toLocaleString()} items in history`
          }}</span>
          <button type="button" :disabled="!clipCount" @click="clearClipboard">
            {{ clipConfirm ? 'Click again to delete' : 'Clear history' }}
          </button>
        </div>
      </section>

      <section v-if="snapshot" v-show="!search && active === 'notes'" id="section-notes">
        <h2>Notes</h2>
        <div class="field">
          <span>Shortcut</span>
          <button
            type="button"
            class="recorder"
            :class="{ listening: notesListening }"
            @click="notesListening = true"
          >
            {{
              notesListening
                ? 'Press a shortcut, or Backspace for none'
                : snapshot.notesShortcut
                  ? formatShortcut(snapshot.notesShortcut)
                  : 'None'
            }}
          </button>
        </div>
        <div class="field inline">
          <button type="button" @click="openNotes()">Open notes</button>
          <button type="button" @click="revealNotes">Show notes folder</button>
        </div>
      </section>

      <SyncSection v-if="snapshot" v-show="!search && active === 'sync'" />

      <section v-if="snapshot" v-show="!search && active === 'claude'" id="section-claude">
        <h2>Claude</h2>
        <div class="field inline">
          <span class="hint-text">
            <template v-if="!cli">Checking for the claude command…</template>
            <template v-else-if="!cli.path">
              claude command not found. Set its path below.
            </template>
            <template v-else>
              {{ cli.version ?? 'claude' }} ·
              {{
                cli.loggedIn
                  ? `signed in (${cli.authMethod ?? 'account'})`
                  : 'not signed in: run "claude auth login" in a terminal'
              }}
            </template>
          </span>
          <button type="button" @click="loadCli">Check again</button>
        </div>
        <label class="field">
          <span>claude command path</span>
          <input
            :value="snapshot.claude.binary"
            type="text"
            spellcheck="false"
            :placeholder="cli?.path ?? '~/.local/bin/claude'"
            @change="
              saveClaudeLimits({ binary: ($event.target as HTMLInputElement).value });
              loadCli();
            "
          />
        </label>

        <h3>Projects</h3>
        <article
          v-for="(project, index) in snapshot.claude.projects"
          :key="project.id"
          class="destination"
        >
          <div class="destination-row">
            <div>
              <strong>{{ project.alias || project.folder.split('/').pop() }}</strong>
              <span class="hint-text">{{ project.folder }}</span>
              <span v-if="index < 8" class="badge">Ctrl+{{ index + 1 }}</span>
            </div>
            <div class="row-actions">
              <button type="button" :disabled="busy" @click="removeProject(project.id)">
                Remove
              </button>
            </div>
          </div>
          <div class="editor">
            <label class="field">
              <span>Alias</span>
              <input
                :value="project.alias"
                type="text"
                placeholder="e.g. zephyr"
                @change="
                  updateProject(project.id, { alias: ($event.target as HTMLInputElement).value })
                "
              />
            </label>
            <label class="field">
              <span>Permissions</span>
              <select
                :value="project.profile"
                @change="
                  updateProject(project.id, {
                    profile: ($event.target as HTMLSelectElement).value as ClaudeProject['profile'],
                  })
                "
              >
                <option value="edit">Edit files here; other commands ask</option>
                <option value="auto">Auto: Claude Code decides</option>
                <option value="plan">Plan only</option>
              </select>
            </label>
            <label class="field">
              <span>Note for every task</span>
              <textarea
                class="import-box"
                rows="2"
                :value="project.note"
                placeholder="e.g. Run the tests before finishing. Use tabs."
                @change="
                  updateProject(project.id, { note: ($event.target as HTMLTextAreaElement).value })
                "
              />
            </label>
            <template v-if="project.allow.length">
              <span class="hint-text">Always allowed here</span>
              <ul class="folders">
                <li v-for="rule in project.allow" :key="rule">
                  <span class="path">{{ rule }}</span>
                  <button
                    type="button"
                    :disabled="busy"
                    @click="
                      updateProject(project.id, {
                        allow: project.allow.filter((item) => item !== rule),
                      })
                    "
                  >
                    Remove
                  </button>
                </li>
              </ul>
            </template>
          </div>
        </article>
        <form class="field inline" @submit.prevent="addProject">
          <input
            v-model="newProjectFolder"
            type="text"
            placeholder="Project folder, e.g. ~/code/zephyr"
          />
          <button type="submit" :disabled="busy || !newProjectFolder.trim()">Add project</button>
          <button type="button" :disabled="busy" @click="chooseProjectFolder">
            Choose folder…
          </button>
        </form>
        <p class="lede">
          Commit and push always ask. Reset, force-push and rm -rf are always blocked.
        </p>

        <h3>Limits</h3>
        <label class="field">
          <span>Task time limit (minutes)</span>
          <input
            :value="snapshot.claude.timeoutMinutes"
            type="number"
            min="1"
            max="600"
            @change="
              saveClaudeLimits({
                timeoutMinutes: Number(($event.target as HTMLInputElement).value),
              })
            "
          />
        </label>
        <label class="field">
          <span>Permission timeout (minutes)</span>
          <input
            :value="snapshot.claude.approvalMinutes"
            type="number"
            min="1"
            max="120"
            @change="
              saveClaudeLimits({
                approvalMinutes: Number(($event.target as HTMLInputElement).value),
              })
            "
          />
        </label>
        <label class="check">
          <input
            type="checkbox"
            :checked="snapshot.claude.notifications"
            :disabled="busy"
            @change="saveClaudeLimits({ notifications: !snapshot.claude.notifications })"
          />
          Notifications
        </label>
      </section>

      <section v-if="snapshot" v-show="!search && active === 'shell'" id="section-shell">
        <h2>Shell</h2>
        <p class="lede">
          Type &gt; or !sh in the bar to run a command there. Ctrl+Enter opens it in a terminal
          instead, for commands that ask for input.
        </p>
        <label v-if="shell?.hasWindowsTerminal" class="field">
          <span>Ctrl+Enter opens commands in</span>
          <select :value="snapshot.shell.terminal" :disabled="busy" @change="changeTerminal">
            <option value="auto">Windows Terminal (a new tab)</option>
            <option value="console">A console window</option>
          </select>
        </label>
        <label v-if="shell?.available.length" class="field">
          <span>Run commands with</span>
          <select :value="snapshot.shell.program" :disabled="busy" @change="changeShell">
            <option value="auto">Automatic (Git Bash if installed, otherwise PowerShell)</option>
            <option
              v-for="item in SHELLS.filter((entry) => shell?.available.includes(entry.id))"
              :key="item.id"
              :value="item.id"
            >
              {{ item.label }}
            </option>
          </select>
        </label>
        <p class="hint-text">
          <template v-if="shell?.using">Commands run in {{ shell.using }}.</template>
          <template v-else>No shell was found.</template>
        </p>
        <div class="field inline">
          <span class="hint-text">{{ snapshot.shell.history.length }} recent commands saved</span>
          <button
            type="button"
            :disabled="busy || !snapshot.shell.history.length"
            @click="forgetCommands"
          >
            Forget recent commands
          </button>
        </div>
      </section>

      <section
        v-if="snapshot"
        v-show="!search && active === 'destinations'"
        id="section-destinations"
      >
        <h2>Destinations</h2>
        <p class="lede">
          Pinned destinations get Ctrl+1–8. Templates take <code>{query}</code>,
          <code>{clipboard}</code>, <code>{date}</code> and <code>{argument}</code>.
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
              <button
                type="button"
                :disabled="busy || index === 0"
                @click="move(destination.id, -1)"
              >
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
              <span>Template</span>
              <input v-model="draft.urlTemplate" type="text" spellcheck="false" required />
            </label>
            <label v-if="destination.kind !== 'ai'" class="field">
              <span>Suggestions</span>
              <select v-model="draft.suggest">
                <option value="none">None</option>
                <option value="google">Google</option>
                <option value="youtube">YouTube</option>
                <option value="wikipedia">Wikipedia</option>
                <option value="pubmed">PubMed</option>
                <option value="custom">Custom…</option>
              </select>
            </label>
            <template v-if="draft.suggest === 'custom'">
              <label class="field">
                <span>Suggestion URL</span>
                <input
                  v-model="draft.suggestUrl"
                  type="text"
                  spellcheck="false"
                  placeholder="https://example.com/complete?q={query}"
                  required
                />
              </label>
              <label class="field">
                <span>JSON path</span>
                <input
                  v-model="draft.suggestPath"
                  type="text"
                  spellcheck="false"
                  placeholder="1, or items.*.title"
                />
              </label>
            </template>
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
            <span>Template</span>
            <input v-model="added.urlTemplate" type="text" spellcheck="false" required />
          </label>
          <label class="field">
            <span>Suggestions</span>
            <select v-model="added.suggest">
              <option value="none">None</option>
              <option value="google">Google</option>
              <option value="youtube">YouTube</option>
              <option value="wikipedia">Wikipedia</option>
              <option value="pubmed">PubMed</option>
              <option value="custom">Custom…</option>
            </select>
          </label>
          <template v-if="added.suggest === 'custom'">
            <label class="field">
              <span>Suggestion URL</span>
              <input
                v-model="added.suggestUrl"
                type="text"
                spellcheck="false"
                placeholder="https://example.com/complete?q={query}"
                required
              />
            </label>
            <label class="field">
              <span>JSON path</span>
              <input
                v-model="added.suggestPath"
                type="text"
                spellcheck="false"
                placeholder="1, or items.*.title"
              />
            </label>
          </template>
          <button class="primary" type="submit" :disabled="busy">Add destination</button>
        </form>

        <div class="field inline">
          <button type="button" @click="copyDestinations">Copy all as JSON</button>
          <button type="button" @click="importOpen = !importOpen">Import JSON…</button>
        </div>
        <form v-if="importOpen" class="editor" @submit.prevent="runImport">
          <label class="field">
            <span>Paste exported destinations</span>
            <textarea
              v-model="importText"
              class="import-box"
              rows="6"
              spellcheck="false"
              placeholder='[{"name": "Arxiv", "triggers": ["arxiv"], "urlTemplate": "https://arxiv.org/a/{query}"}]'
            />
          </label>
          <div class="row-actions">
            <button class="primary" type="submit" :disabled="busy || !importText.trim()">
              Import
            </button>
            <button type="button" @click="importOpen = false">Cancel</button>
          </div>
        </form>
      </section>

      <section v-if="snapshot" v-show="!search && active === 'files'" id="section-files">
        <h2>Files</h2>
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
          <button type="button" :disabled="busy" @click="chooseRoot">Choose folder…</button>
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

      <section v-if="snapshot" v-show="!search && active === 'history'" id="section-history">
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
        <button
          type="button"
          :disabled="busy || snapshot.history.length === 0"
          @click="wipeHistory"
        >
          Clear history
        </button>
      </section>
    </div>
    <SelectMenu :root="settingsEl" />
  </main>
</template>
