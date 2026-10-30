<script setup lang="ts">
  import './settings.css';
  import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
  import { getVersion } from '@tauri-apps/api/app';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import {
    checkForUpdates,
    rehearseUpdate,
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
    saveTheme,
    saveBugCapture,
    saveShowStats,
    saveVolumeKeys,
    saveVolumeOverlay,
    saveWatchConnection,
    saveCpuTemp,
    cpuTempStatus,
    openOnboarding,
    hideSettings,
    windowChrome,
    captureRecording,
    saveOnboardingEveryLaunch,
    permissionRequest,
    revealCaptures,
    saveShellProgram,
    shellInfo,
    saveNotesShortcut,
    errorMessage,
    fileIndexStatus,
    getSnapshot,
    rebuildFileIndex,
    saveFileFolders,
    saveSettings,
  } from '../api';
  import SyncSection from './SyncSection.vue';
  import PermissionsSection from './PermissionsSection.vue';
  import LaunchGroupsSection from './LaunchGroupsSection.vue';
  import { THEMES } from '../themes';
  import { isMac } from '../keys';
  import { usePoll } from '../poll';
  import { useWindowListener } from '../windowEvents';
  import Keys from '../components/Keys.vue';
  import ShortcutRecorder from '../components/ShortcutRecorder.vue';
  import InfoTip from '../components/InfoTip.vue';
  import SpeedPanel from '../components/SpeedPanel.vue';
  import { useSpeedTest } from '../speedTest';
  import { vTip } from '../tip';
  import ShortcutsSection from './ShortcutsSection.vue';
  import BootChoice from './BootChoice.vue';
  import WeatherSection from './WeatherSection.vue';
  import SelectMenu from '../components/SelectMenu.vue';
  import { GLYPHS } from '../bar/glyphs';
  import { lastTestLine } from '../bar/network';
  import {
    isSnapshot,
    type ClipboardSettings,
    type FileIndexStatus,
    type ShellInfo,
    type ShellProgram,
    type Snapshot,
  } from '../types';

  const snapshot = ref<Snapshot | null>(null);
  const appVersion = ref('');
  void getVersion()
    .then((version) => (appVersion.value = version))
    .catch(() => undefined);
  const status = ref<string | null>(null);
  const formError = ref<string | null>(null);
  const updateMessage = ref<string | null>(null);
  const busy = ref(false);

  const fileStatus = ref<FileIndexStatus | null>(null);
  const newRoot = ref('');
  const newExclude = ref('');

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

  let unlistens: UnlistenFn[] = [];
  let statusTimer = 0;

  // Saved roots, or the default (home) the index reports when none are saved.
  const fileRoots = computed(() => snapshot.value?.fileRoots ?? fileStatus.value?.roots ?? []);

  const indexSummary = computed(() => {
    const current = fileStatus.value;
    if (!current) return '';
    if (current.roots.length === 0) return 'File search is off. Add a folder to turn it on.';
    const count = current.entries.toLocaleString();
    return current.scanning ? `Indexing… ${count} items so far` : `${count} items indexed`;
  });

  useWindowListener('keydown', onShortcutKey, true);
  useWindowListener('keydown', onEscape);

  onMounted(async () => {
    await refresh();
    pageReady();
    void loadClip();
    void loadShell();
    unlistens.push(
      await listen<unknown>('state-changed', (event) => {
        if (isSnapshot(event.payload)) snapshot.value = event.payload;
      })
    );
    // The window is built ahead of time and only hidden on close, so opening it is just
    // showing it: jump to the asked-for section (the bar's !set names one; by URL when it
    // creates this window), then catch up on data.
    unlistens.push(
      await listen<string>('settings-shown', (event) => {
        // Opened plainly, it starts at General; it never remembers the last page.
        if (event.payload) void revealSection(event.payload);
        else open('general');
        window.setTimeout(() => void refresh(), 50);
      })
    );
    const section = new URLSearchParams(window.location.search).get('section');
    if (section) void revealSection(section);
    void pollFileStatus();
  });

  onUnmounted(() => {
    window.clearTimeout(statusTimer);
    for (const unlisten of unlistens) unlisten();
  });

  // One page at a time, picked from the sidebar: the app's own settings, then its utilities
  // (each tool that has settings, A to Z, so a new one joins on its own), then About and Dev
  // at the bottom.
  interface Section {
    id: string;
    label: string;
    icon: string;
    group?: 'utilities';
    pinned?: boolean;
    /** Words that find the page in search besides its label and its controls' own. */
    keywords?: string;
  }
  const SECTIONS = [
    { id: 'general', label: 'General', icon: 'gear' },
    { id: 'theme', label: 'Theme', icon: 'palette' },
    { id: 'shortcuts', label: 'Shortcuts', icon: 'keyboard' },
    { id: 'launchgroups', label: 'Launch groups', icon: 'grid' },
    { id: 'files', label: 'Files', icon: 'folder' },
    { id: 'sync', label: 'Sync', icon: 'sync' },
    ...(isMac ? [{ id: 'permissions', label: 'Permissions', icon: 'shield' }] : []),
    { id: 'clipboard', label: 'Clipboard', icon: 'clipboard', group: 'utilities' },
    { id: 'resources', label: 'Resources', icon: 'cpu', group: 'utilities' },
    ...(isMac
      ? []
      : [
          {
            id: 'sound',
            label: 'Sound',
            icon: 'speaker',
            group: 'utilities',
            keywords: 'volume keys overlay mixer',
          },
        ]),
    { id: 'terminal', label: 'Terminal', icon: 'prompt', group: 'utilities' },
    { id: 'notes', label: 'Notes', icon: 'note', group: 'utilities' },
    {
      id: 'network',
      label: 'Network',
      icon: 'pulse',
      group: 'utilities',
      keywords: 'connection ping latency speed test',
    },
    {
      id: 'weather',
      label: 'Weather',
      icon: 'weatherPartly',
      group: 'utilities',
      keywords: 'location city celsius fahrenheit units',
    },
    { id: 'about', label: 'About', icon: 'info', pinned: true },
    { id: 'dev', label: 'Dev', icon: 'code', pinned: true },
  ] as Section[];
  const MAIN = SECTIONS.filter((section) => !section.group && !section.pinned);
  const UTILITIES = SECTIONS.filter((section) => section.group === 'utilities').sort((a, b) =>
    a.label.localeCompare(b.label)
  );
  const PINNED = SECTIONS.filter((section) => section.pinned);
  /** The sidebar's order, for ↑ and ↓. */
  const NAV = [...MAIN, ...UTILITIES, ...PINNED];
  const active = ref('general');

  // The title strip the page draws in place of the system's: a press drags the window, a
  // double-click maximizes it, and on Windows it has the window buttons.
  const isWindows = navigator.userAgent.includes('Windows');
  function onTitleDown(event: MouseEvent) {
    if (event.button === 0 && event.detail === 1) void windowChrome('drag');
  }

  // Bug captures work here as in the bar (Ctrl+R records this window, Ctrl+Shift+S snapshots
  // it); the title strip shows a recording running, or a snapshot just taken.
  const recording = ref(false);
  const snapped = ref(false);
  let captureStarted = 0;
  let snappedTimer = 0;
  void captureRecording()
    .then((on) => (recording.value = on))
    .catch(() => undefined);
  void listen<boolean>('capture-state', (event) => {
    if (event.payload) {
      captureStarted = Date.now();
      recording.value = true;
      return;
    }
    recording.value = false;
    // A snapshot starts and stops within a moment; a recording is stopped by hand.
    if (Date.now() - captureStarted < 1500) {
      snapped.value = true;
      window.clearTimeout(snappedTimer);
      snappedTimer = window.setTimeout(() => (snapped.value = false), 1800);
    }
  }).then((unlisten) => unlistens.push(unlisten));

  // Below 640 px the sidebar is a rail of icons, and their names are tooltips.
  const narrowQuery = window.matchMedia('(max-width: 639px)');
  const narrow = ref(narrowQuery.matches);
  narrowQuery.addEventListener('change', (event) => (narrow.value = event.matches));

  /** ↑ and ↓ move between the sidebar's rows and open each. */
  function onNavKey(event: KeyboardEvent) {
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
    event.preventDefault();
    const at = NAV.findIndex((section) => section.id === active.value);
    const next = NAV[(at + (event.key === 'ArrowDown' ? 1 : -1) + NAV.length) % NAV.length];
    open(next.id);
    void nextTick(() =>
      document.querySelector<HTMLElement>(`[data-section="${next.id}"]`)?.focus()
    );
  }
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
      if (
        section.label.toLowerCase().includes(query) ||
        section.keywords?.split(' ').some((word) => word.startsWith(query))
      ) {
        found.push({ section: section.id, label: section.label, element: root });
      }
      const labels = root.querySelectorAll<HTMLElement>(
        '.field > span:first-child, label.check, h3, button'
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
      (result.element.closest('.field, .check, h3') as HTMLElement | null) ?? result.element;
    flash(target);
  }

  /** Scrolls a control into view and flashes it, so the eye lands on it. */
  function flash(target: HTMLElement) {
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

  /** Opens a section, and with "section:control" also scrolls to that control and flashes
   *  it, as a search result does (the bar's "Zephyr Summon Shortcut" row). */
  async function revealSection(value: string) {
    const [section, focus] = value.split(':');
    if (SECTIONS.some((item) => item.id === section)) open(section);
    await nextTick();
    if (!focus) return;
    const target = document.querySelector<HTMLElement>(
      `#section-${section} [data-focus="${CSS.escape(focus)}"]`
    );
    if (target) flash(target);
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

  /** Saves a change while the controls wait; true once the new state is in. */
  async function save(request: Promise<Snapshot>): Promise<boolean> {
    busy.value = true;
    try {
      apply(await request);
      return true;
    } catch (error) {
      formError.value = errorMessage(error);
      return false;
    } finally {
      busy.value = false;
    }
  }

  // Esc closes Settings, unless something open took it first: a dropdown, a shortcut
  // being recorded, or search text (which Esc clears).
  function onEscape(event: KeyboardEvent) {
    if (event.key !== 'Escape' || event.defaultPrevented || event.isComposing) return;
    if (search.value) {
      search.value = '';
      return;
    }
    event.preventDefault();
    void hideSettings();
  }

  function onShortcutKey(event: KeyboardEvent) {
    // ⌘F / Ctrl+F jumps to the settings search.
    if ((event.metaKey || event.ctrlKey) && event.code === 'KeyF') {
      event.preventDefault();
      searchEl.value?.focus();
      searchEl.value?.select();
      return;
    }
  }

  /** The new key caps say it took; only a failure (taken, or not a shortcut) says more, at
   *  the recorder itself. */
  const summonError = ref<string | null>(null);

  async function commitShortcut(shortcut: string) {
    const current = snapshot.value;
    if (!current) return;
    busy.value = true;
    try {
      apply(await saveSettings(shortcut, current.launchAtStartup));
      summonError.value = null;
    } catch (error) {
      summonError.value = errorMessage(error);
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

  // Dev builds can't install a real update; this plays the update card through instead.
  const isDev = import.meta.env.DEV;
  const devBuild = isDev ? __DEV_BUILD__ : '';
  async function rehearse() {
    try {
      await rehearseUpdate();
      updateMessage.value = 'Rehearsing a made-up update; watch the corner of your screen.';
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
    const saved = await save(saveFileFolders(roots, excludes));
    // The new walk starts on a worker thread; give it a moment to report.
    if (saved) window.setTimeout(() => void pollFileStatus(), 300);
    return saved;
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

  const RESUME = [
    { seconds: 0, label: 'Nothing: start empty' },
    { seconds: 30, label: '30 seconds' },
    { seconds: 120, label: '2 minutes' },
    { seconds: 600, label: '10 minutes' },
    { seconds: 3600, label: 'An hour' },
    { seconds: 86400, label: 'Always' },
  ];

  async function chooseTheme(id: string) {
    await save(saveTheme(id));
  }

  async function toggleOnboardingEveryLaunch() {
    if (!snapshot.value) return;
    try {
      apply(await saveOnboardingEveryLaunch(!snapshot.value.onboardingEveryLaunch));
    } catch (error) {
      formError.value = errorMessage(error);
    }
  }

  async function toggleVolumeKeys() {
    if (snapshot.value) await save(saveVolumeKeys(!snapshot.value.volumeKeys));
  }

  const OVERLAY_PLACES = [
    { id: 'bottom', label: 'Bottom center' },
    { id: 'top', label: 'Top center' },
    { id: 'bottom-right', label: 'Bottom right' },
  ];
  async function changeOverlayPlace(event: Event) {
    await save(saveVolumeOverlay((event.target as HTMLSelectElement).value));
  }

  async function toggleStats() {
    if (snapshot.value) await save(saveShowStats(!snapshot.value.showStats));
  }

  // Windows CPU temperature: turning it on or off waits on Windows' permission prompt; while
  // Resources is open, the line under it says what the helper last read.
  const cpuTempBusy = ref(false);
  const cpuTempError = ref('');
  const cpuTempNow = ref<number | null>(null);
  // The helper's copy is missing or from an older Zephyr; setting it up again refreshes it.
  const cpuTempStale = ref(false);
  async function toggleCpuTemp() {
    if (snapshot.value) await setCpuTemp(!snapshot.value.cpuTemp);
  }
  async function setCpuTemp(next: boolean) {
    cpuTempBusy.value = true;
    cpuTempError.value = '';
    try {
      apply(await saveCpuTemp(next));
      cpuTempStale.value = false;
    } catch (error) {
      cpuTempError.value = errorMessage(error);
    } finally {
      cpuTempBusy.value = false;
    }
  }
  // The reading every 2 seconds while its page shows; a closed Settings is only hidden.
  usePoll(
    () =>
      cpuTempStatus()
        .then((status) => {
          cpuTempNow.value = status.celsius;
          cpuTempStale.value = status.needsSetup;
        })
        .catch(() => undefined),
    2000,
    () => !isMac && active.value === 'resources' && !!snapshot.value?.cpuTemp
  );
  const cpuTempLine = computed(() => {
    if (cpuTempBusy.value) return 'Waiting for Windows to ask for permission…';
    if (cpuTempError.value) return cpuTempError.value;
    if (!snapshot.value?.cpuTemp) {
      return 'Off. Some anti-cheat and antivirus tools are wary of hardware drivers.';
    }
    if (cpuTempStale.value && cpuTempNow.value === null) {
      return 'The helper needs setting up again for this version of Zephyr.';
    }
    return cpuTempNow.value !== null
      ? `Reading ${Math.round(cpuTempNow.value)} °C`
      : 'Starting the helper…';
  });

  async function toggleWatch() {
    if (snapshot.value) await save(saveWatchConnection(!snapshot.value.watchConnection));
  }

  // The speed test from Settings, shown as it runs like in the Network view. The result also
  // lands in the state, so the last-test line below updates too.
  const speed = useSpeedTest();
  const { running: speedRunning, testing: speedTesting, result: speedResult } = speed;
  const speedError = ref('');
  async function runSpeed() {
    speedError.value = '';
    speedError.value = await speed.run();
  }

  async function toggleBugCapture() {
    if (snapshot.value) await save(saveBugCapture(!snapshot.value.bugCapture));
  }

  async function showCaptures() {
    try {
      await revealCaptures();
    } catch (error) {
      formError.value = errorMessage(error);
    }
  }

  async function changeResume(event: Event) {
    await save(saveResumeSeconds(Number((event.target as HTMLSelectElement).value)));
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

  async function chooseRoot() {
    const folder = await pickFolder().catch(() => null);
    if (!folder || !snapshot.value) return;
    await saveFolders([...fileRoots.value, folder], snapshot.value.fileExcludes);
  }

  async function saveNotesKey(shortcut: string) {
    await save(saveNotesShortcut(shortcut));
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
    if (await save(saveClipboardSettings({ ...snapshot.value.clipboard, ...partial }))) {
      void loadClip();
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

  // On a Mac this is the Permissions page's Allow: it clears a stale grant and opens the
  // Accessibility list; the page then shows it turning on.
  async function allowPasting() {
    if (!isMac) {
      clipPasteAllowed.value = await clipRequestPastePermission();
      return;
    }
    await permissionRequest('accessibility').catch(() => undefined);
    open('permissions');
  }
</script>

<template>
  <main ref="settingsEl" class="settings">
    <!-- The toolbar: moves the window (a double-click maximizes it), with the title over the
         sidebar, the search at the pages' edge, and on Windows the window buttons. -->
    <header class="settings-titlebar" @mousedown="onTitleDown" @dblclick="windowChrome('maximize')">
      <span class="settings-title">Settings</span>
      <div class="settings-find" @mousedown.stop @dblclick.stop>
        <svg class="settings-find-icon" viewBox="0 0 24 24" aria-hidden="true">
          <circle cx="11" cy="11" r="6.5" />
          <path d="M16 16.5 20 20.5" />
        </svg>
        <input
          ref="searchEl"
          v-model="search"
          class="settings-search"
          type="search"
          placeholder="Search settings"
          aria-label="Search settings"
          spellcheck="false"
          @keydown="onSearchKey"
        />
        <Keys v-if="!search" class="settings-find-keys" keys="mod+f" small />
      </div>
      <span v-if="snapped" class="capture-note">Snapshot saved and copied</span>
      <span v-else-if="recording" class="capture-note recording">
        Recording · {{ isMac ? '⌃R' : 'Ctrl+R' }} stops
      </span>
      <div v-if="isWindows" class="window-buttons" @mousedown.stop @dblclick.stop>
        <button type="button" aria-label="Minimize" @click="windowChrome('minimize')">
          <svg viewBox="0 0 10 10" aria-hidden="true"><path d="M0 5h10" /></svg>
        </button>
        <button type="button" aria-label="Maximize" @click="windowChrome('maximize')">
          <svg viewBox="0 0 10 10" aria-hidden="true">
            <rect x=".5" y=".5" width="9" height="9" />
          </svg>
        </button>
        <button type="button" class="close" aria-label="Close" @click="windowChrome('close')">
          <svg viewBox="0 0 10 10" aria-hidden="true"><path d="M0 0l10 10M10 0 0 10" /></svg>
        </button>
      </div>
    </header>
    <!-- The sidebar's glass, behind the toolbar's title too; the rows sit over it. -->
    <div class="settings-side" aria-hidden="true" />
    <nav class="settings-nav" aria-label="Settings" @keydown="onNavKey">
      <template v-for="section in MAIN" :key="section.id">
        <button
          v-tip="narrow ? section.label : null"
          type="button"
          class="settings-row"
          :class="{ chosen: !search && active === section.id }"
          :data-section="section.id"
          :aria-current="!search && active === section.id ? 'page' : undefined"
          @click="open(section.id)"
        >
          <svg viewBox="0 0 24 24" aria-hidden="true" v-html="GLYPHS[section.icon]" />
          <span>{{ section.label }}</span>
        </button>
      </template>
      <h3 class="settings-group">Utilities</h3>
      <button
        v-for="section in UTILITIES"
        :key="section.id"
        v-tip="narrow ? section.label : null"
        type="button"
        class="settings-row"
        :class="{ chosen: !search && active === section.id }"
        :data-section="section.id"
        :aria-current="!search && active === section.id ? 'page' : undefined"
        @click="open(section.id)"
      >
        <svg viewBox="0 0 24 24" aria-hidden="true" v-html="GLYPHS[section.icon]" />
        <span>{{ section.label }}</span>
      </button>
      <div class="settings-pinned">
        <button
          v-for="section in PINNED"
          :key="section.id"
          v-tip="narrow ? section.label : null"
          type="button"
          class="settings-row quiet"
          :class="{ chosen: !search && active === section.id }"
          :data-section="section.id"
          :aria-current="!search && active === section.id ? 'page' : undefined"
          @click="open(section.id)"
        >
          <svg viewBox="0 0 24 24" aria-hidden="true" v-html="GLYPHS[section.icon]" />
          <span>{{ section.label }}</span>
        </button>
      </div>
    </nav>
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
        <div class="field" data-focus="summon">
          <span>Summon shortcut</span>
          <ShortcutRecorder
            :keys="snapshot.summonShortcut"
            @record="commitShortcut"
            @invalid="summonError = $event"
          />
          <p v-if="summonError" class="notice">{{ summonError }}</p>
        </div>
        <div class="field" data-focus="startup">
          <span>Run at boot</span>
          <BootChoice :snapshot="snapshot" @apply="apply" />
        </div>
        <label class="field">
          <span>When Zephyr opens</span>
          <select
            :value="snapshot.returnToMode ? 'return' : 'search'"
            :disabled="busy"
            @change="
              save(saveOpenBehavior(($event.target as HTMLSelectElement).value === 'return'))
            "
          >
            <option value="return">Return to what I was doing, for 5 minutes</option>
            <option value="search">Always start at the search bar</option>
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
        <div class="field inline">
          <button type="button" @click="openOnboarding">Run setup again</button>
        </div>
      </section>

      <section v-if="snapshot" v-show="!search && active === 'theme'" id="section-theme">
        <h2>Theme</h2>
        <div class="field" data-focus="theme">
          <div class="themes compact" role="radiogroup" aria-label="Theme">
            <button
              v-for="theme in THEMES"
              :key="theme.id"
              type="button"
              class="theme"
              role="radio"
              :aria-checked="snapshot.theme === theme.id"
              :class="{ chosen: snapshot.theme === theme.id }"
              :disabled="busy"
              @click="chooseTheme(theme.id)"
            >
              <span class="theme-swatch" :style="{ background: theme.tint }" aria-hidden="true">
                <span class="theme-row" />
                <span class="theme-row selected" :style="{ background: `${theme.accent}26` }" />
                <span class="theme-row" />
                <span class="theme-dot" :style="{ background: theme.accent }" />
              </span>
              {{ theme.name }}
            </button>
          </div>
        </div>
      </section>

      <section v-if="snapshot" v-show="!search && active === 'about'" id="section-about">
        <h2>About</h2>
        <div class="field" data-focus="updates">
          <span
            >Zephyr
            {{ isDev ? ['dev', devBuild].filter(Boolean).join(' · ') : appVersion || '…' }}</span
          >
          <button type="button" @click="updates">Check for updates</button>
          <span v-if="updateMessage" class="hint-text">{{ updateMessage }}</span>
        </div>
      </section>

      <section v-if="snapshot" v-show="!search && active === 'dev'" id="section-dev">
        <h2>Dev</h2>
        <h3>Bug reports</h3>
        <label class="check">
          <input
            type="checkbox"
            :checked="snapshot.bugCapture"
            :disabled="busy"
            @change="toggleBugCapture"
          />
          {{ isMac ? '⌃R' : 'Ctrl+R' }} records and {{ isMac ? '⌃⇧S' : 'Ctrl+Shift+S' }} snapshots
          the Zephyr window in front
          <InfoTip text="Only Zephyr's windows are captured, never other apps." />
        </label>
        <div class="field inline">
          <button type="button" @click="showCaptures">Show captures</button>
        </div>
        <template v-if="isDev">
          <h3>Dev build</h3>
          <label class="check">
            <input
              type="checkbox"
              :checked="snapshot.onboardingEveryLaunch"
              :disabled="busy"
              @change="toggleOnboardingEveryLaunch"
            />
            Show setup on every launch
          </label>
          <div class="field inline">
            <button type="button" @click="rehearse">Rehearse an update</button>
          </div>
        </template>
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
          <ShortcutRecorder
            :keys="snapshot.clipboard.shortcut"
            @record="saveClip({ shortcut: $event })"
            @invalid="formError = $event"
          />
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
          <ShortcutRecorder
            :keys="snapshot.notesShortcut"
            clearable
            @record="saveNotesKey"
            @invalid="formError = $event"
          />
        </div>
        <div class="field inline">
          <button type="button" @click="openNotes()">Open notes</button>
          <button type="button" @click="revealNotes">Show notes folder</button>
        </div>
      </section>

      <SyncSection v-if="snapshot" v-show="!search && active === 'sync'" />
      <LaunchGroupsSection
        v-if="snapshot"
        v-show="!search && active === 'launchgroups'"
        :snapshot="snapshot"
        :shown="!search && active === 'launchgroups'"
        @apply="apply"
      />
      <section v-if="snapshot" v-show="!search && active === 'resources'" id="section-resources">
        <h2>Resources</h2>
        <label class="check">
          <input
            type="checkbox"
            :checked="snapshot.showStats"
            :disabled="busy"
            @change="toggleStats"
          />
          Show CPU, GPU and memory in the status bar
          <InfoTip text="A click on them opens System Resources." />
        </label>
        <template v-if="!isMac">
          <label class="check">
            <input
              type="checkbox"
              :checked="snapshot.cpuTemp"
              :disabled="busy || cpuTempBusy"
              @change="toggleCpuTemp"
            />
            Show CPU temperature
            <InfoTip
              text="Windows only lets a driver read the CPU's temperature. This installs PawnIO, a signed open-source driver, and a small Zephyr helper that runs as administrator to read it. Windows asks for permission once, and again to remove the helper when you turn it off."
            />
          </label>
          <p class="hint-text cpu-temp-line">{{ cpuTempLine }}</p>
          <button
            v-if="snapshot.cpuTemp && cpuTempStale && cpuTempNow === null"
            type="button"
            :disabled="cpuTempBusy"
            @click="setCpuTemp(true)"
          >
            Set up again
          </button>
        </template>
      </section>

      <section v-if="snapshot && !isMac" v-show="!search && active === 'sound'" id="section-sound">
        <h2>Sound</h2>
        <label class="check">
          <input
            type="checkbox"
            :checked="snapshot.volumeKeys"
            :disabled="busy"
            @change="toggleVolumeKeys"
          />
          Volume keys show Zephyr's overlay
          <InfoTip
            text="Replaces Windows' volume flyout. The keys change master; Alt with a volume key picks an app, which the keys change until the overlay fades. With the bar open, the levels show in the bar instead."
          />
        </label>
        <label class="field">
          <span>Overlay shows at</span>
          <select
            :value="snapshot.volumeOverlay"
            :disabled="busy || !snapshot.volumeKeys"
            @change="changeOverlayPlace"
          >
            <option v-for="place in OVERLAY_PLACES" :key="place.id" :value="place.id">
              {{ place.label }}
            </option>
          </select>
        </label>
      </section>
      <section v-if="snapshot" v-show="!search && active === 'network'" id="section-network">
        <h2>Network</h2>
        <label class="check">
          <input
            type="checkbox"
            :checked="snapshot.watchConnection"
            :disabled="busy"
            @change="toggleWatch"
          />
          Watch connection
          <InfoTip text="Pings your router and 1.1.1.1 once a second, about 1 MB an hour." />
        </label>
        <div class="speed-field">
          <span class="speed-text">
            <b>
              Speed test
              <InfoTip
                text="Download and upload speed, and how much slower the connection reacts while busy (bufferbloat). About 15 seconds, up to 250 MB."
              />
            </b>
            <span class="hint-text">
              {{
                speedRunning
                  ? 'Testing, about 15 seconds…'
                  : speedError ||
                    (snapshot.lastSpeed ? lastTestLine(snapshot.lastSpeed) : 'Not run yet')
              }}
            </span>
          </span>
          <button v-if="speedRunning" type="button" @click="speed.cancel">Cancel</button>
          <button v-else type="button" @click="runSpeed">Run speed test</button>
        </div>
        <SpeedPanel :testing="speedTesting" :result="speedResult" />
      </section>

      <section v-if="snapshot" v-show="!search && active === 'terminal'" id="section-terminal">
        <h2>Terminal</h2>
        <label v-if="shell?.available.length" class="field" data-focus="shell">
          <span class="field-label">
            Run commands with
            <InfoTip text="For commands typed after > in the bar, and Shell." />
          </span>
          <select :value="shell.program" :disabled="busy" @change="changeShell">
            <option
              v-for="item in SHELLS.filter((entry) => shell?.available.includes(entry.id))"
              :key="item.id"
              :value="item.id"
            >
              {{ item.label }}
            </option>
          </select>
        </label>
        <p v-else class="hint-text">Commands run in your login shell.</p>
      </section>

      <WeatherSection
        v-if="snapshot"
        v-show="!search && active === 'weather'"
        :snapshot="snapshot"
        @apply="apply"
      />
      <ShortcutsSection
        v-if="snapshot"
        v-show="!search && active === 'shortcuts'"
        :snapshot="snapshot"
        @apply="apply"
      />
      <PermissionsSection
        v-if="snapshot && SECTIONS.some((section) => section.id === 'permissions')"
        v-show="!search && active === 'permissions'"
        :active="!search && active === 'permissions'"
      />

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
    </div>
    <SelectMenu :root="settingsEl" />
  </main>
</template>
