<script setup lang="ts">
  import './bar.css';
  import {
    computed,
    defineAsyncComponent,
    nextTick,
    onMounted,
    onUnmounted,
    reactive,
    ref,
    shallowRef,
    watch,
  } from 'vue';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import {
    dispatch,
    errorMessage,
    recordViewOpen,
    getSnapshot,
    hideBar,
    launchApp,
    quickLook,
    pageReady,
    editBinding,
    windowFocus,
    windowClose,
    appQuit,
    openFile,
    openSetting,
    openNotes,
    openSettings,
    updateInstallNow,
    updateWaiting,
    setActiveMode,
    revealFile,
    setBarHeight,
    startBarDrag,
    barMove,
    barMoveEnd,
    launchGroup,
    prepareGrid,
    warmSuggestions,
    allowProtectedFolders,
    captureEvent,
    systemStats,
    captureRecording,
    openUrl,
    resourcesQuit,
  } from '../api';
  import ResourcesView from './ResourcesView.vue';
  import NetworkView from './NetworkView.vue';
  import NetworkCard from './NetworkCard.vue';
  import PortCard from './PortCard.vue';
  import { useNetworkSummary } from './network';
  import DeviceCard from './DeviceCard.vue';
  import ResultList from './ResultList.vue';
  import FooterWeather from './FooterWeather.vue';
  import TickerCard from './TickerCard.vue';
  import WeatherCard from './WeatherCard.vue';
  import ActionMenu from '../components/ActionMenu.vue';
  import { useActionMenu, type MenuAction } from '../components/actionMenu';
  import { vTip } from '../tip';
  import { isMac } from '../keys';
  import { barShown } from '../visibility';
  import { useLater } from '../poll';
  import { useWindowListener } from '../windowEvents';
  import NowPlayingStrip from './NowPlayingStrip.vue';
  import { dragOut } from './dragOut';
  import { GLYPHS } from './glyphs';
  import { useNowPlaying } from './nowPlaying';
  import { useFooterStats } from './footerStats';
  import { bindingForAlias, bindingTarget, buildRows, groups, rowKey, type RowCtx } from './rows';
  import { googleUrl } from './google';
  import { useDevices } from './useDevices';
  import { usePower } from './usePower';
  import { useRunKey } from './useRunKey';
  import { useSuggest } from './useSuggest';
  import { useTypeAhead } from './useTypeAhead';
  import { useTickerCard } from './useTickerCard';
  import { useWeatherCard } from './useWeatherCard';
  import {
    isSnapshot,
    type Binding,
    type Snapshot,
    type SuggestionRow,
    type SystemStats,
    type SuggestResponse,
    type PortUse,
  } from '../types';
  // Views used now and then load on demand, and are fetched in the background on the first
  // summon, so startup parses less and opening one later doesn't wait.
  import type ClipboardViewComponent from './ClipboardView.vue';
  import type TypingViewComponent from '../typing/TypingView.vue';
  import type ShellViewComponent from '../shell/ShellView.vue';
  const loadClipboard = () => import('./ClipboardView.vue');
  const loadTyping = () => import('../typing/TypingView.vue');
  const loadShell = () => import('../shell/ShellView.vue');
  const loadPreview = () => import('./FilePreviewPane.vue');
  const ClipboardView = defineAsyncComponent(loadClipboard);
  const TypingView = defineAsyncComponent(loadTyping);
  const ShellView = defineAsyncComponent(loadShell);
  const FilePreviewPane = defineAsyncComponent(loadPreview);
  let prefetched = false;
  function prefetchViews() {
    if (prefetched) return;
    prefetched = true;
    for (const load of [loadClipboard, loadTyping, loadShell, loadPreview]) {
      void load().catch(() => (prefetched = false));
    }
  }

  const root = ref<HTMLElement | null>(null);
  const inputEl = ref<HTMLInputElement | null>(null);
  // Replaced whole on every change, never edited in place, so neither needs deep tracking.
  const snapshot = shallowRef<Snapshot | null>(null);
  const query = ref('');
  // A search narrowed to one kind of result (files, bookmarks, settings, apps). It shows as a
  // label before the text, and is sent to Rust as the bang it stands for, which still works
  // typed: "!f " becomes the Files label.
  interface Scope {
    bang: string;
    name: string;
  }
  const SCOPES: Record<string, Scope> = {
    f: { bang: 'f', name: 'Files' },
    file: { bang: 'f', name: 'Files' },
    files: { bang: 'f', name: 'Files' },
    bm: { bang: 'bm', name: 'Bookmarks' },
    bookmark: { bang: 'bm', name: 'Bookmarks' },
    bookmarks: { bang: 'bm', name: 'Bookmarks' },
    set: { bang: 'set', name: 'Settings' },
    settings: { bang: 'set', name: 'Settings' },
    app: { bang: 'app', name: 'Apps' },
    apps: { bang: 'app', name: 'Apps' },
  };
  const scope = ref<Scope | null>(null);
  /** What Rust sees: the text, behind its scope's bang when there is one. */
  function typedText(): string {
    return scope.value ? `!${scope.value.bang} ${query.value}` : query.value;
  }
  // The last suggest reply and the text it was for; the rows are built from it, so command
  // rows and job counts stay current without asking again.
  const lastResponse = shallowRef<SuggestResponse | null>(null);
  const lastQuery = ref('');
  const mode = ref<'search' | 'recent'>('recent');
  const notice = ref<string | null>(null);
  const selected = ref(-1);
  const loadError = ref<string | null>(null);

  // Views that take over the bar. Workspaces (typing and Shell) stick: every open lands back
  // in them until Esc leaves. Esc to the main bar only parks a workspace; a second Esc there
  // clears it. Clipboard is a picker: it sticks until something is picked.
  type View = 'root' | 'clip' | 'typing' | 'shell' | 'resources' | 'network';
  const WORKSPACES = ['typing', 'shell'];
  const MODE_NAMES: Record<string, string> = {
    typing: 'typing test',
    shell: 'Shell',
  };
  const view = ref<View>('root');
  const parked = ref('');
  // A view is mounted on first use and then kept, so its state survives leaving it.
  const visited = reactive<Record<string, boolean>>({});
  const clipMode = computed(() => view.value === 'clip');
  const typingMode = computed(() => view.value === 'typing');
  const shellMode = computed(() => view.value === 'shell');
  // Shell commands, from > or !sh; a command never runs on unscoped Enter.
  const shellQuery = ref('');
  const shellView = ref<InstanceType<typeof ShellViewComponent> | null>(null);
  const SHELL_PREFIX = /^\s*(?:>|!(?:sh|shell)(?:\s|$))\s*/i;
  const clipQuery = ref('');
  const resourcesView = ref<InstanceType<typeof ResourcesView> | null>(null);
  const networkView = ref<InstanceType<typeof NetworkView> | null>(null);
  const clipView = ref<InstanceType<typeof ClipboardViewComponent> | null>(null);
  const typingView = ref<InstanceType<typeof TypingViewComponent> | null>(null);
  // Other computers asking to join sync; checked when the bar opens and every minute.
  const devices = useDevices(notice);
  const { devicesWaiting, answerDevice } = devices;
  // Alt+1..9 (⌘1..9 on a Mac) runs the Nth row.
  const runKey = useRunKey();
  const { RUN_KEY, showNumbers } = runKey;

  // Reopening soon after closing returns to the view and text it closed on.
  let hiddenAt = 0;
  let hiddenAtWall = 0;
  /** How long the bar has been hidden: the monotonic clock, unless the wall clock saw more
   *  (the webview's clock can stand still while the computer sleeps). */
  function hiddenFor(): number {
    if (!hiddenAt) return 0;
    return Math.max(performance.now() - hiddenAt, Date.now() - hiddenAtWall);
  }
  // After this long hidden, the bar opens fresh on the main bar instead of where it was.
  const FRESH_AFTER_MS = 5 * 60 * 1000;

  // Ctrl+K's menu, over the results.
  const menu = useActionMenu(() => actions.value);
  const { open: actionsOpen, index: actionIndex } = menu;

  let userMoved = false;
  // Arrow presses that land before the first rows do.
  let pendingMove = 0;
  let unlistens: UnlistenFn[] = [];
  // A list caught up a moment after a quit or close; dropped if the bar goes first.
  const later = useLater();
  let observer: ResizeObserver | null = null;

  const rowCtx = computed<RowCtx>(() => ({
    snapshot: snapshot.value,
    parked: parked.value,
    parkedName: MODE_NAMES[parked.value] ?? '',
    typed: scope.value ? '' : lastQuery.value,
  }));

  // Power commands ("shutdown in 30m"), and the first Enter of one waiting for its second.
  const { powerRow, confirmArm, powerRows, runPower, cancelPower } = usePower({
    snapshot,
    query,
    notice,
  });

  const { scheduleSuggest } = useSuggest({
    text: typedText,
    shown: () => lastResponse.value?.items ?? [],
    powerRow,
    apply: applySuggestions,
    notice,
    trace,
  });

  const built = computed(() => {
    if (!lastResponse.value) return { rows: [] as SuggestionRow[], preselect: null };
    const result = buildRows(lastResponse.value, rowCtx.value);
    // Power rows lead; a command named outright ("shutdown") takes Enter.
    const power = powerRows();
    if (power.length) {
      result.rows.unshift(...power);
      const named = power.findIndex((row) => row.kind === 'power' && row.power?.exact);
      result.preselect =
        named >= 0 ? named : result.preselect === null ? null : result.preselect + power.length;
    }
    if (looksLikeCommand(lastQuery.value)) {
      result.rows.push({
        label: 'Run in shell',
        hint: '> or !sh',
        query: '',
        target: 'shell',
        kind: 'view',
        section: 'shell',
        icon: 'glyph:prompt',
      });
    }
    return result;
  });
  const items = computed(() => built.value.rows);
  const sections = computed(() => groups(items.value));

  const selectedItem = computed(() =>
    selected.value >= 0 ? items.value[selected.value] : undefined
  );

  // Type-ahead: the rest of what Enter would act on, drawn faintly after the cursor.
  const typeAhead = useTypeAhead({
    query,
    inputEl,
    snapshot,
    scoped: () => scope.value !== null,
    items,
    selectedItem,
  });
  const { ghost, trackCaret } = typeAhead;

  /** The drag-to-move key: ⌘ on a Mac, where Ctrl+click is a right-click; Ctrl elsewhere. */
  function moveKeyOnly(event: KeyboardEvent | MouseEvent): boolean {
    const key = isMac ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey;
    return key && !event.altKey && !event.shiftKey;
  }

  /** ⌘-drag (Ctrl-drag on Windows) anywhere on the bar moves it; plain clicks and drags
   *  work as before. */
  function dragWithCtrl(event: MouseEvent) {
    if (event.button !== 0 || !moveKeyOnly(event)) return;
    event.preventDefault();
    event.stopPropagation();
    const fromX = event.screenX;
    const fromY = event.screenY;
    void startBarDrag()
      .then((start) => {
        // On a Mac the page moves the bar: it follows the pointer once a frame, and the
        // grid shows where it snaps until the button comes up.
        if (start) followPointer(fromX, fromY, start);
      })
      .catch((error) => {
        console.error('Ctrl+drag failed', error);
        notice.value = `Couldn't move the bar: ${errorMessage(error)}`;
      });
  }

  function followPointer(fromX: number, fromY: number, [left, top, scale]: number[]) {
    let next: [number, number] | null = null;
    let frame = 0;
    const move = (event: MouseEvent) => {
      next = [
        Math.round(left + (event.screenX - fromX) * scale),
        Math.round(top + (event.screenY - fromY) * scale),
      ];
      if (!frame) {
        frame = requestAnimationFrame(() => {
          frame = 0;
          if (next) void barMove(next[0], next[1]).catch(() => undefined);
        });
      }
    };
    const drop = () => {
      window.removeEventListener('mousemove', move, true);
      window.removeEventListener('mouseup', drop, true);
      cancelAnimationFrame(frame);
      const last = next;
      void (last ? barMove(last[0], last[1]) : Promise.resolve())
        .then(() => barMoveEnd())
        .catch(() => undefined);
    };
    window.addEventListener('mousemove', move, true);
    window.addEventListener('mouseup', drop, true);
  }

  // Bug captures (Ctrl+R and Ctrl+Shift+S, unless turned off in Settings): while one runs,
  // the bar logs what it does.
  const capturing = ref(false);
  function trace(kind: string, data: unknown) {
    if (capturing.value) {
      void captureEvent(kind, data).catch(() => undefined);
    }
  }
  void captureRecording()
    .then((on) => (capturing.value = on))
    .catch(() => undefined);
  void listen<boolean>('capture-state', (event) => {
    capturing.value = event.payload;
    // Everything a snapshot needs to explain its one frame, and where a recording begins.
    if (event.payload) {
      trace('start', {
        text: query.value,
        completion: ghost.value,
        mode: mode.value,
        view: view.value,
        selected: selected.value,
        notice: notice.value,
        rows: items.value.map((item) => ({
          section: item.section,
          kind: item.kind,
          label: item.label,
          hint: item.hint,
        })),
      });
    }
  }).then((unlisten) => unlistens.push(unlisten));
  watch(query, (text) => trace('text', { text }));
  watch(selected, (index) => trace('selected', { index, label: items.value[index]?.label }));
  watch(notice, (text) => text && trace('notice', { text }));

  // Ctrl+K (⌘K on a Mac) lists what else the selected row, or the typed text, can do.
  const rowActions = computed<MenuAction[]>(() => {
    const item = selectedItem.value;
    const list: MenuAction[] = [];
    if (item?.portUse) {
      list.push(...portActions(item.portUse));
    } else if (item?.kind === 'answer') {
      list.push({ label: 'Copy answer', run: () => copyText(item.label) });
    } else if (item?.kind === 'netCard') {
      if (item.target === 'ip') {
        list.push({ label: `Copy ${item.label}`, keys: 'enter', run: () => copyText(item.label) });
      }
      list.push({ label: 'Open Network', run: () => openView('network') });
      list.push({ label: 'Run speed test', run: () => openView('speedtest') });
    } else if (item?.kind === 'app' && item.appId && item.windows?.length) {
      // A running app: switch to any of its windows, start another, or close it.
      const appId = item.appId;
      const front = item.windows[0].handle;
      for (const open of item.windows.slice(0, 5)) {
        list.push({ label: `Switch to ${open.title}`, run: () => switchTo(open.handle) });
      }
      list.push({ label: 'Open a new window', run: () => launch(appId, true) });
      if (item.privateWindow) {
        const privateWindow = item.privateWindow;
        list.push({
          label: `Open ${privateWindow} window`,
          keys: 'shift+enter',
          run: () => launch(appId, false, true),
        });
      }
      list.push({ label: 'Close window', run: () => closeWindow(front) });
      list.push({ label: `Quit ${item.label}`, run: () => quitApp(appId, item.label) });
    } else if (item?.kind === 'app' && item.appId) {
      const appId = item.appId;
      list.push({ label: `Open ${item.label}`, run: () => launch(appId) });
      if (item.privateWindow) {
        list.push({
          label: `Open ${item.privateWindow} window`,
          keys: 'shift+enter',
          run: () => launch(appId, false, true),
        });
      }
      if (item.running) {
        list.push({ label: `Quit ${item.label}`, run: () => quitApp(appId, item.label) });
      }
      list.push({ label: 'Copy name', run: () => copyText(item.label) });
    } else if (item?.kind === 'window' && item.window) {
      const handle = item.window;
      list.push({ label: 'Switch', run: () => switchTo(handle) });
      list.push({ label: 'Close window', run: () => closeWindow(handle) });
      if (item.appId) {
        const appId = item.appId;
        list.push({ label: `Quit ${item.hint}`, run: () => quitApp(appId, item.hint) });
      }
    } else if (item?.kind === 'file' && item.path) {
      const path = item.path;
      list.push({ label: `Open ${item.label}`, run: () => openPath(path) });
      list.push({
        label: isMac ? 'Quick Look' : previewOpen.value ? 'Close preview' : 'Preview',
        run: () => togglePreview(),
      });
      list.push({ label: `Show in ${isMac ? 'Finder' : 'Explorer'}`, run: () => reveal(path) });
      list.push({ label: 'Copy path', run: () => copyText(path) });
    } else if (item?.kind === 'setting' && item.settingId) {
      const settingId = item.settingId;
      list.push({ label: `Open ${item.label}`, run: () => openPage(settingId) });
    } else {
      const text = item ? item.query : query.value.trim();
      if (!text) return list;
      const link = (item ? item.target : lastResponse.value?.target) === 'url';
      list.push({ label: link ? 'Open link' : 'Search Google', run: () => run(text, false) });
      list.push({ label: 'Copy link', run: () => copyText(link ? text : googleUrl(text)) });
      list.push({ label: 'Copy text', run: () => copyText(text) });
    }
    return list;
  });

  // Anything that can have a hotkey or alias gets one more action, which opens Settings on it.
  const actions = computed<MenuAction[]>(() => {
    const item = selectedItem.value;
    const bindable = item ? bindingTarget(item) : null;
    if (!bindable) return rowActions.value;
    const set = snapshot.value?.bindings?.find((binding) => binding.target === bindable.target);
    const label = set ? 'Change hotkey or alias…' : 'Set hotkey or alias…';
    return [
      ...rowActions.value,
      { label, run: () => editBinding(bindable.target, bindable.label) },
    ];
  });

  // CPU, GPU, memory and disk as meters in a slim footer, from readings the app sends
  // twice a second while the bar shows (Settings › General can turn it off).
  const footerStats = useFooterStats();
  const meters = footerStats.footer;
  const statsShown = computed(() => snapshot.value?.showStats !== false && meters.value !== null);

  /** Only the main bar shows the footer; other views let readings pass. */
  function takeStats(next: SystemStats) {
    if (snapshot.value?.showStats === false || view.value !== 'root') return;
    footerStats.push(next);
  }

  // The network card ("ping", "ip"): its numbers refresh each second while it shows, and
  // the footer's chip says when the line is in trouble (net.rs decides when).
  const netCardShown = computed(
    () => view.value === 'root' && items.value.some((row) => row.kind === 'netCard')
  );
  const netSummary = useNetworkSummary(netCardShown);
  const netAlert = computed(() => (statsShown.value ? (meters.value?.network ?? null) : null));
  const netAlertText = computed(() => {
    const alert = netAlert.value;
    if (!alert) return '';
    if (alert.kind === 'offline') return 'Offline';
    if (alert.kind === 'drop') return 'Dropping';
    return `${Math.round(alert.ms)} ms`;
  });

  // Port rows ("port 3000", "ports"): Enter shows the process in Resources; Ctrl+K quits it
  // (a second Enter confirms, as in Resources), copies its PID or opens its local page.
  const resourcesFilter = ref('');
  function showInResources(pid: number) {
    query.value = '';
    resourcesFilter.value = String(pid);
    enter('resources');
  }

  function portName(use: PortUse): string {
    return use.name || `PID ${use.pid}`;
  }

  /** The quit waiting for Enter on this row, if any. */
  function armedFor(use: PortUse | undefined): 'quit' | 'force' | null {
    const [kind, what, pid] = confirmArm.value.split(':');
    if (kind !== 'port' || !use || String(use.pid) !== pid) return null;
    return what === 'force' ? 'force' : 'quit';
  }

  function armQuit(use: PortUse, force: boolean) {
    confirmArm.value = `port:${force ? 'force' : 'quit'}:${use.pid}`;
    // The card says so itself; a plain row needs the line.
    notice.value =
      selectedItem.value?.kind === 'portCard'
        ? null
        : `Press Enter to ${force ? 'force quit' : 'quit'} ${portName(use)}, or Esc to back out`;
  }

  const portConfirmText = computed(() => {
    const use = selectedItem.value?.portUse;
    const armed = armedFor(use);
    if (!use || !armed) return '';
    return `${armed === 'force' ? 'Force quit' : 'Quit'} ${portName(use)}?`;
  });

  async function quitPort(use: PortUse, force: boolean) {
    confirmArm.value = '';
    try {
      await resourcesQuit(portName(use), [use.pid], force);
      notice.value = `${force ? 'Force quit' : 'Quit'} ${portName(use)}.`;
      later(scheduleSuggest, 400);
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  function portActions(use: PortUse): MenuAction[] {
    const name = portName(use);
    const list: MenuAction[] = [
      { label: 'Show in Resources', keys: 'enter', run: () => showInResources(use.pid) },
      { label: `Quit ${name}`, run: () => armQuit(use, false) },
      { label: `Force quit ${name}`, run: () => armQuit(use, true) },
      { label: 'Copy process ID', run: () => copyText(String(use.pid)) },
    ];
    if (use.protocol === 'TCP' && use.state === 'LISTEN') {
      const url = `http://localhost:${use.port}`;
      list.push({
        label: `Open ${url}`,
        run: () =>
          openUrl(url)
            .then(clearAfterOpen)
            .catch((error) => (notice.value = errorMessage(error))),
      });
    }
    if (use.command) list.push({ label: 'Copy command', run: () => copyText(use.command) });
    return list;
  }

  // What's playing, as a small chip at the end of the search bar.
  const media = useNowPlaying();
  const playingNow = computed(() => media.shown.value);

  // An update downloaded and put off with Later: a button beside Settings installs it.
  const updateReady = ref<string | null>(null);
  void updateWaiting()
    .then((version) => (updateReady.value = version))
    .catch(() => undefined);
  function installUpdate() {
    updateReady.value = null;
    void updateInstallNow().catch((error) => (notice.value = errorMessage(error)));
  }

  // The page's keys from the start, ahead of anything the app sends.
  useWindowListener('keydown', onKey, true);
  useWindowListener('keydown', trackModifiers, true);
  useWindowListener('keyup', trackModifiers, true);
  useWindowListener('keyup', onKeyUp, true);
  useWindowListener('blur', onBlur);
  // A drop lands outside the page's key events, so the pointer corrects what's held.
  useWindowListener('mousemove', trackModifiers, true);

  function onShown() {
    barShown.value = true;
    void updateWaiting()
      .then((version) => (updateReady.value = version))
      .catch(() => undefined);
    resumeTimers();
    prefetchViews();
    sentHeight = 0;
    // Opens the connection to the suggestion service before the first keystroke.
    void warmSuggestions().catch(() => undefined);
    void devices.loadDevices();
    // Hidden over five minutes: start over on the main bar, as with returning to modes
    // turned off.
    const stale = hiddenFor() > FRESH_AFTER_MS;
    if (stale && parked.value) void setActiveMode('').catch(() => undefined);
    if (stale || (snapshot.value && !snapshot.value.returnToMode)) {
      parked.value = '';
      view.value = 'root';
      resetForSummon();
      scheduleSuggest();
      void focusInput();
      return;
    }
    // Still inside a view (a workspace, or a picker mid-search): reopen right there.
    if (view.value === 'resources') resourcesView.value?.start();
    if (view.value === 'network') networkView.value?.start();
    // Copies made while the bar was hidden.
    if (view.value === 'clip') void clipView.value?.refresh();
    if (view.value !== 'root') return;
    if (WORKSPACES.includes(parked.value)) {
      enter(parked.value as View);
      return;
    }
    // The main bar keeps its text only briefly.
    const limit = (snapshot.value?.resumeSeconds ?? 120) * 1000;
    const recent = hiddenAt > 0 && limit > 0 && hiddenFor() < limit;
    // A scoped search (files, apps, settings, notes) sticks until something is picked.
    const picker = scope.value !== null || /^!(note|notes)\b/i.test(query.value);
    if (!recent && !picker) resetForSummon();
    // The empty bar's list changes while hidden (apps found after startup, a launch, a
    // search), and the text is unchanged, so nothing else would refresh it.
    if (!query.value) scheduleSuggest();
    void focusInput();
  }

  function onHidden() {
    barShown.value = false;
    hiddenAt = performance.now();
    pauseTimers();
    hiddenAtWall = Date.now();
    resourcesView.value?.stop();
    networkView.value?.stop();
    typingView.value?.pause();
  }

  onMounted(async () => {
    // Listened for all at once, while the settings load.
    const listening = Promise.all([
      listen('bar-shown', onShown),
      listen<string | null>('update-waiting', (event) => (updateReady.value = event.payload)),
      // Startup's unseen draw, again once the apps are found, so the first summon has them.
      listen('bar-redraw', () => {
        if (!query.value) scheduleSuggest();
      }),
      listen('bar-hidden', onHidden),
      listen<SystemStats>('stats', (event) => takeStats(event.payload)),
      // A zephyr:// link: text to type in, or why the link was refused. Links never run
      // anything from the bar (search links open their page from the app), and the app
      // refuses shell text.
      listen<{ query: string; notice: string | null }>('bar-input', (event) => {
        const { query: text, notice: refused } = event.payload;
        if (refused) {
          notice.value = refused;
          return;
        }
        query.value = text;
        void focusInput();
      }),
      listen('clipboard-shown', () => openClip('')),
      // A hotkey the user gave a mode or scoped search (Settings > Shortcuts).
      listen<string>('bar-open', (event) => {
        query.value = '';
        openView(event.payload);
      }),
      listen('bar-focus', () => {
        void focusInput();
      }),
      listen<unknown>('state-changed', (event) => {
        if (!isSnapshot(event.payload)) return;
        snapshot.value = event.payload;
      }),
    ]);
    try {
      const loaded = await getSnapshot();
      // A change that arrived meanwhile is newer.
      snapshot.value ??= loaded;
      const remembered = snapshot.value.activeMode;
      if (snapshot.value.returnToMode && MODE_NAMES[remembered]) {
        parked.value = remembered;
        if (WORKSPACES.includes(remembered)) enter(remembered as View);
      }
    } catch (error) {
      loadError.value = errorMessage(error);
    }
    void devices.loadDevices(true);
    unlistens.push(...(await listening));

    if (root.value) {
      observer = new ResizeObserver(() => syncHeight());
      observer.observe(root.value);
      syncHeight();
    }
    // One sample now, so the footer is already there the first time the bar opens instead of
    // popping in a second later.
    if (snapshot.value?.showStats !== false) {
      void systemStats()
        .then((next) => {
          if (!meters.value) takeStats(next);
        })
        .catch(() => undefined);
    }
    await focusInput();
    scheduleSuggest();
    // Drawn with its content, the bar goes back off screen until it's summoned (window.rs).
    pageReady();
  });

  /** Alt+Tab away: the Alt reached the bar, its release didn't. */
  function onBlur() {
    movable.value = false;
    closeOnEscapeUp = false;
    runKey.reset();
  }

  // Timers that only matter while the bar shows: the check for computers asking to join sync,
  // and what's-playing's clock. They stop while it's hidden, as does the countdown's clock.
  function pauseTimers() {
    devices.pause();
    media.pause();
  }

  function resumeTimers() {
    devices.resume();
    media.resume();
  }
  onUnmounted(() => {
    observer?.disconnect();
    for (const unlisten of unlistens) unlisten();
  });

  watch(selected, () => {
    actionsOpen.value = false;
    // A quit waiting for Enter belongs to the row it was asked on.
    if (confirmArm.value.startsWith('port:')) {
      confirmArm.value = '';
      notice.value = null;
    }
  });

  /** Views the empty bar can offer, counted each time one opens so the busiest come first. */
  const COUNTED_VIEWS = ['clip', 'network', 'resources', 'shell', 'typing'];

  watch(view, async () => {
    if (COUNTED_VIEWS.includes(view.value)) {
      void recordViewOpen(view.value).catch(() => undefined);
    }
    await nextTick();
    syncHeight();
    if (view.value === 'root') {
      scheduleSuggest();
      void focusInput();
    } else if (view.value === 'shell') {
      void shellView.value?.focus();
    }
  });

  /** Clipboard history with `text` searched; already open, it starts over with the latest. */
  function openClip(text: string) {
    clipQuery.value = text;
    if (view.value === 'clip') void clipView.value?.reopen(text);
    else enter('clip');
  }

  /** Enters a view. Workspaces become the place the bar returns to. */
  function enter(next: View) {
    actionsOpen.value = false;
    visited[next] = true;
    view.value = next;
    if (WORKSPACES.includes(next) && parked.value !== next) {
      parked.value = next;
      void setActiveMode(next).catch(() => undefined);
    }
  }

  /** Enters the shell carrying text; a shell kept from earlier takes the new text too. */
  function enterShell(text: string) {
    shellQuery.value = text;
    if (shellView.value && text) shellView.value.text = text;
    enter('shell');
  }

  /** Esc or Backspace out of a view: back to the main bar, with a workspace left parked. */
  function leave() {
    view.value = 'root';
  }

  /** System Resources' "Search the web": a Google search, from the main bar. */
  function searchFromResources(name: string) {
    leave();
    void run(name, false);
  }

  /** Set by Esc's press at the root; its release closes the bar. Only that same press may:
   *  every other Esc press clears it, so the one that backs out of a view never also closes. */
  let closeOnEscapeUp = false;

  function onKeyUp(event: KeyboardEvent) {
    if (event.key !== 'Escape' || !closeOnEscapeUp) return;
    closeOnEscapeUp = false;
    if (view.value !== 'root') return;
    event.preventDefault();
    closeFromRoot();
  }

  /** Esc on the main bar: forget the parked workspace, then close. */
  function closeFromRoot() {
    if (parked.value) {
      parked.value = '';
      void setActiveMode('').catch(() => undefined);
    }
    void hideBar();
  }

  // A pasted line that reads like a command gets a "Run in shell" row; Enter still goes to
  // the web unless that row is picked.
  const COMMAND_START =
    /^(?:git|gh|ls|ll|cd|cat|echo|grep|rg|find|curl|wget|npm|npx|bun|bunx|pnpm|yarn|node|deno|python3?|py|pip3?|uv|cargo|rustup|go|make|docker|kubectl|ssh|scp|rsync|brew|winget|choco|scoop|sudo|chmod|chown|mkdir|rm|mv|cp|touch|tail|head|ps|kill|df|du|code|wsl|pwsh|powershell|tar|unzip|ping|nslookup|ipconfig|ifconfig|[A-Z][a-z]+-[A-Z]\w+)$/;

  function looksLikeCommand(text: string): boolean {
    const line = text.trim().replace(/^\$\s+/, '');
    if (!line || line.startsWith('!')) return false;
    if (/^(?:\.{1,2}|~)\//.test(line)) return true;
    const [first, ...rest] = line.split(/\s+/);
    return rest.length > 0 && (COMMAND_START.test(first) || /\s(?:\||&&)\s/.test(line));
  }

  function openView(id: string) {
    const typed = query.value;
    query.value = '';
    const back = id === 'back';
    if (back) id = parked.value;
    if (id === 'shell') {
      enterShell(back ? '' : typed.trim().replace(/^\$\s+/, ''));
    } else if (id === 'typing') {
      enter(id);
    } else if (id === 'clip') {
      openClip('');
    } else if (id === 'network') {
      enter('network');
    } else if (id === 'speedtest') {
      // Into the Network view, where the test shows; a view kept open already has it.
      enter('network');
      void nextTick(() => networkView.value?.runSpeedTest());
    } else if (id === 'resources') {
      resourcesFilter.value = '';
      enter('resources');
    } else if (id === 'notes') {
      // Notes opens its own windows rather than a view, so it's counted here.
      void recordViewOpen('notes').catch(() => undefined);
      void openNotes();
    } else if (id === 'ip') {
      query.value = 'ip';
      void focusInput();
    } else if (id.startsWith('group:')) {
      void launchGroup(id.slice(6)).catch((error) => (notice.value = errorMessage(error)));
    } else if (id.startsWith('scope:')) {
      scope.value = SCOPES[id.slice(6)] ?? null;
      void focusInput();
    }
  }

  watch(query, () => {
    if (!scope.value && view.value === 'root' && runAlias()) return;
    const scoped = /^!([a-z]+)\s+(.*)$/is.exec(query.value);
    if (!scope.value && scoped && SCOPES[scoped[1].toLowerCase()]) {
      scope.value = SCOPES[scoped[1].toLowerCase()];
      query.value = scoped[2];
      return;
    }
    if (/^!type$/i.test(query.value.trim()) && query.value.endsWith(' ')) {
      query.value = '';
      enter('typing');
      return;
    }
    // > opens the shell at once; !sh and !shell wait for a space so !shop still works.
    const shell = SHELL_PREFIX.exec(query.value);
    if (shell && (query.value.trimStart().startsWith('>') || /\s$/.test(shell[0]))) {
      const text = query.value.slice(shell[0].length);
      query.value = '';
      enterShell(text);
      return;
    }
    const clip = /^!clip(?:\s+(.*))?$/i.exec(query.value);
    if (clip) {
      query.value = '';
      openClip(clip[1] ?? '');
      return;
    }
    actionsOpen.value = false;
    confirmArm.value = '';
    // The highlight stays put until this text's results replace it, so it doesn't blink.
    userMoved = false;
    notice.value = null;
    scheduleSuggest();
  });

  watch(scope, (next) => {
    // File search is the first use of Desktop, Documents and Downloads, so macOS asks now,
    // in context, rather than at launch.
    if (next?.bang === 'f' && snapshot.value && !snapshot.value.protectedFolders) {
      void allowProtectedFolders().catch(() => undefined);
    }
    selected.value = -1;
    userMoved = false;
    scheduleSuggest();
  });

  /** Clears the main bar's text and selection; views and the parked workspace are untouched. */
  function resetForSummon() {
    query.value = '';
    scope.value = null;
    selected.value = -1;
    notice.value = null;
  }

  async function focusInput() {
    await nextTick();
    inputEl.value?.focus();
    inputEl.value?.select();
  }

  // Every view is the bar's one fixed size, so the window (and its blur) always matches the
  // panel exactly. Views used to widen the window without widening the panel.
  // The last height sent, so an unchanged layout doesn't cross to Rust again; forgotten on
  // each summon, in case the window was resized meanwhile.
  let sentHeight = 0;
  function syncHeight() {
    if (!root.value) return;
    const rect = root.value.getBoundingClientRect();
    const height = Math.ceil(rect.height);
    if (height === sentHeight) return;
    sentHeight = height;
    trace('size', { height, width: Math.ceil(rect.width) });
    void setBarHeight(height).catch(() => (sentHeight = 0));
  }

  /** Shows a suggest reply, keeping the row the user moved to selected when late results
   *  reshuffle the list. */
  async function applySuggestions(
    response: SuggestResponse,
    includeRemote: boolean,
    typed: string
  ) {
    const kept = userMoved && selectedItem.value ? rowKey(selectedItem.value) : null;
    lastResponse.value = response;
    lastQuery.value = query.value;
    mode.value = response.mode;
    notice.value = response.notice;
    const index = kept ? items.value.findIndex((row) => rowKey(row) === kept) : -1;
    if (index >= 0) {
      selected.value = index;
    } else {
      userMoved = false;
      selected.value = built.value.preselect ?? -1;
    }
    if (pendingMove) {
      const delta = pendingMove;
      pendingMove = 0;
      moveSelection(delta);
    }
    // The row list is only built when a capture is recording.
    if (capturing.value)
      trace('results', {
        text: typed,
        remote: includeRemote,
        selected: selected.value,
        rows: items.value.map((item) => ({
          section: item.section,
          kind: item.kind,
          label: item.label,
        })),
      });
    await nextTick();
    syncHeight();
  }

  /** What's playing: Ctrl+Space plays or pauses anywhere in the bar; with nothing typed,
   *  Ctrl+Shift+→/← skip (with text they still select by word). Whether it took the key. */
  function mediaKey(event: KeyboardEvent): boolean {
    if (!playingNow.value || !event.ctrlKey || event.altKey || event.metaKey) return false;
    if (event.code === 'Space' && !event.shiftKey) {
      event.preventDefault();
      if (!event.repeat) media.press('toggle');
      return true;
    }
    if (
      event.shiftKey &&
      view.value === 'root' &&
      !query.value &&
      (event.key === 'ArrowRight' || event.key === 'ArrowLeft')
    ) {
      event.preventDefault();
      if (!event.repeat) media.press(event.key === 'ArrowRight' ? 'next' : 'previous');
      return true;
    }
    return false;
  }

  /** The view taking over the bar, for its keys. */
  function viewShowing(): { onKey: (event: KeyboardEvent) => void } | null {
    switch (view.value) {
      case 'clip':
        return clipView.value;
      case 'network':
        return networkView.value;
      case 'resources':
        return resourcesView.value;
      case 'typing':
        return typingView.value;
      case 'shell':
        return shellView.value;
      default:
        return null;
    }
  }

  function onKey(event: KeyboardEvent) {
    trace('key', {
      key: event.key,
      ctrl: event.ctrlKey,
      alt: event.altKey,
      shift: event.shiftKey,
      meta: event.metaKey,
    });
    // WebKit ends a composition before the key that commits it, which then reads 229.
    if (event.isComposing || event.keyCode === 229) return;
    // A fresh Esc press closes the bar on release only if the root takes it, below.
    if (event.key === 'Escape' && !event.repeat) closeOnEscapeUp = false;
    // A computer asking to join sync: never plain Enter, only ⌘Y and ⌘N.
    if (devices.onKey(event)) return;
    if (mediaKey(event)) return;
    // A view that takes over the bar takes every other key.
    if (view.value !== 'root') {
      viewShowing()?.onKey(event);
      return;
    }
    if (event.key === 'Enter' && /^!(?:sh|shell)$/i.test(query.value.trim())) {
      event.preventDefault();
      query.value = '';
      enterShell('');
      return;
    }
    // !type then Enter opens the typing test too.
    if (event.key === 'Enter' && /^!type$/i.test(query.value.trim())) {
      event.preventDefault();
      query.value = '';
      enter('typing');
      return;
    }

    const modifier = event.ctrlKey || event.metaKey;
    // Ctrl+S (⌘S on a Mac) opens Settings.
    if (moveKeyOnly(event) && event.code === 'KeyS') {
      event.preventDefault();
      if (!event.repeat) void openSettings();
      return;
    }
    // Ctrl+K and, while its menu is open, the menu's keys; anything else goes back to typing.
    if (menu.onKey(event)) return;

    // Right arrow or End at the end of the text takes the suggested completion.
    if (typeAhead.onKey(event)) return;

    // Shift+Space on a file previews it (plain Space still types); Esc closes the preview
    // before anything else.
    if (
      event.key === ' ' &&
      event.shiftKey &&
      !event.ctrlKey &&
      !event.altKey &&
      !event.metaKey &&
      selectedItem.value?.kind === 'file'
    ) {
      event.preventDefault();
      if (!event.repeat) togglePreview();
      return;
    }
    if (event.key === 'Escape' && previewShown.value) {
      event.preventDefault();
      previewOpen.value = false;
      return;
    }

    // Esc backs out of a shutdown waiting for its second Enter before it closes anything.
    if (event.key === 'Escape' && confirmArm.value) {
      event.preventDefault();
      confirmArm.value = '';
      notice.value = null;
      return;
    }

    // Esc, or Backspace on empty text, drops a scope before anything else.
    if (
      scope.value &&
      (event.key === 'Escape' || (event.key === 'Backspace' && !event.repeat && !query.value))
    ) {
      event.preventDefault();
      scope.value = null;
      return;
    }

    // Esc closes the bar when it's let go, so the release doesn't land in the app underneath
    // (a full-screen video there took it as its own Esc and left full screen).
    if (event.key === 'Escape') {
      event.preventDefault();
      if (!event.repeat) closeOnEscapeUp = true;
      return;
    }

    // Tab would only move focus out of the search box. On the ticker card it steps through the
    // chart's spans.
    if (event.key === 'Tab') {
      event.preventDefault();
      if (cardQuote.value && !modifier && !event.altKey) stepRange(event.shiftKey ? -1 : 1);
      return;
    }

    // Alt+1..9 (⌘1..9 on a Mac) runs the Nth row, as Enter on it would.
    const index = runKey.rowFor(event);
    if (index !== null) {
      if (index < Math.min(items.value.length, visibleCount.value)) {
        event.preventDefault();
        showNumbers.value = false;
        if (!event.repeat) chooseAt(index);
      }
      return;
    }

    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      moveSelection(event.key === 'ArrowDown' ? 1 : -1);
      return;
    }

    // Ctrl+Enter on the weather card opens a full forecast; Enter stays on the search.
    if (
      cardForecast.value &&
      modifier &&
      !event.altKey &&
      !event.shiftKey &&
      event.key === 'Enter'
    ) {
      event.preventDefault();
      if (!event.repeat) openForecast();
      return;
    }

    // Likewise on the ticker card: the full quote.
    if (cardQuote.value && modifier && !event.altKey && !event.shiftKey && event.key === 'Enter') {
      event.preventDefault();
      if (!event.repeat) openQuote();
      return;
    }

    // Ctrl+Enter with the IP card up opens the Network view; Enter copies the address.
    if (
      netCardShown.value &&
      modifier &&
      !event.altKey &&
      !event.shiftKey &&
      event.key === 'Enter'
    ) {
      event.preventDefault();
      if (!event.repeat) openView('network');
      return;
    }

    // File actions: Ctrl+Enter shows the file in its folder, Ctrl+Shift+C copies its path.
    const file = selectedItem.value?.kind === 'file' ? selectedItem.value.path : undefined;
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

    // Shift+Enter on a browser opens it in a private window (Incognito, InPrivate).
    const browser = selectedItem.value?.privateWindow ? selectedItem.value.appId : undefined;
    if (browser && event.key === 'Enter' && event.shiftKey && !modifier && !event.altKey) {
      event.preventDefault();
      if (!event.repeat) void launch(browser, false, true);
      return;
    }

    if (event.key === 'Enter') {
      event.preventDefault();
      if (event.repeat) return;
      if (event.shiftKey) {
        if (!modifier && !event.altKey) void googleAiMode();
        return;
      }
      void accept();
    }
  }

  // The weather card: "weather", "forecast", or either with a city ("weather in tokyo").
  const { forecast, cardForecast, openForecast, showWeather } = useWeatherCard({
    query,
    shown: () => !scope.value && view.value === 'root',
    menuOpen: actionsOpen,
    inputEl,
    notice,
    clearAfterOpen,
  });

  // The ticker card: a stock or crypto typed on its own ("TSLA", "eth", "$aapl").
  const { cardQuote, quoteRange, stepRange, pickRange, openQuote } = useTickerCard({
    query,
    shown: () => !scope.value && view.value === 'root',
    menuOpen: actionsOpen,
    notice,
    clearAfterOpen,
  });

  /**
   * Shift+Enter on a search opens it in Google's AI Mode (udm=50) instead of the usual
   * results. Anything else ignores it.
   */
  async function googleAiMode() {
    const item = selectedItem.value;
    const searchRow = item && ['query', 'history', 'remote'].includes(item.kind) ? item : null;
    const text = (searchRow ? searchRow.query : query.value).trim();
    const target = searchRow ? searchRow.target : lastResponse.value?.target;
    if (!text || target !== 'web' || (item && !searchRow)) return;
    try {
      await openUrl(`${googleUrl(text)}&udm=50`);
      clearAfterOpen();
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  // ⌘ alone on a Mac (Ctrl elsewhere) means "drag to move": the cursor and the search bar
  // say so while it's held.
  const movable = ref(false);
  let gridReady = false;
  function trackModifiers(event: KeyboardEvent | MouseEvent) {
    movable.value = moveKeyOnly(event);
    // The grid's window is made the first time Ctrl is held, so the drag never waits on it.
    if (movable.value && !gridReady) {
      gridReady = true;
      void prepareGrid().catch(() => (gridReady = false));
    }
    runKey.track(event);
  }

  /**
   * Aliases the user set (Settings > Shortcuts). A direct one fires as soon as it's typed;
   * a command's, typed and then a space, goes into that command with whatever follows. An app's or bookmark's alias waits for Enter (its row tops the list instead).
   * Returns whether it took the text.
   */
  function runAlias(): boolean {
    const text = query.value;
    const ctx = rowCtx.value;
    const direct = bindingForAlias(ctx, text);
    if (direct?.direct && text.trim() === text) {
      query.value = '';
      void runBinding(direct, '');
      return true;
    }
    const spaced = /^(\S+) (.*)$/s.exec(text);
    if (!spaced) return false;
    const binding = bindingForAlias(ctx, spaced[1]);
    if (!binding || binding.direct) return false;
    if (!/^(view|scope):/.test(binding.target)) return false;
    void runBinding(binding, spaced[2]);
    return true;
  }

  async function runBinding(binding: Binding, rest: string) {
    const [kind, ...parts] = binding.target.split(':');
    const value = parts.join(':');
    if (kind === 'view' || kind === 'scope') {
      const id = kind === 'scope' ? binding.target : value;
      query.value = rest;
      openView(id);
      if (kind === 'scope') query.value = rest;
    } else if (kind === 'app') {
      await launch(value);
    } else if (kind === 'bookmark') {
      await openUrl(value).catch((error) => (notice.value = errorMessage(error)));
    } else if (kind === 'setting') {
      await openPage(value);
    } else if (kind === 'group') {
      await launchGroup(value).catch((error) => (notice.value = errorMessage(error)));
    }
  }

  async function openNote(item: SuggestionRow) {
    try {
      if (item.kind === 'note') await openNotes(item.noteId);
      else await openNotes(undefined, item.query);
      query.value = '';
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  async function copyText(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      query.value = '';
      await hideBar();
    } catch {
      notice.value = "Couldn't copy it";
    }
  }

  function hover(index: number) {
    if (selected.value === index) return;
    userMoved = true;
    selected.value = index;
  }

  function moveSelection(delta: number) {
    if (items.value.length === 0) {
      pendingMove += delta;
      return;
    }
    userMoved = true;
    // Only rows that fit are shown, so moving never lands on one that isn't.
    const count = Math.min(items.value.length, visibleCount.value);
    if (count === 0) return;
    if (selected.value < 0) {
      selected.value = delta > 0 ? 0 : count - 1;
      return;
    }
    selected.value = (Math.min(selected.value, count - 1) + delta + count) % count;
  }

  async function accept() {
    const item = selected.value >= 0 ? items.value[selected.value] : undefined;
    if (item) {
      if (item.kind === 'answer') {
        await copyText(item.label);
        return;
      }
      if (item.portUse) {
        const armed = armedFor(item.portUse);
        if (armed) await quitPort(item.portUse, armed === 'force');
        else showInResources(item.portUse.pid);
        return;
      }
      if (item.kind === 'netCard') {
        if (item.target === 'ip') await copyText(item.label);
        else openView('network');
        return;
      }
      if (item.kind === 'powerPending' || (item.kind === 'power' && item.power?.cancel)) {
        await cancelPower();
        return;
      }
      if (item.kind === 'power' && item.power) {
        await runPower(item.power);
        return;
      }
      if (item.kind === 'bookmark') {
        try {
          await openUrl(item.query);
          query.value = '';
          notice.value = null;
        } catch (error) {
          notice.value = errorMessage(error);
        }
        return;
      }
      if (item.kind === 'note' || item.kind === 'noteNew') {
        await openNote(item);
        return;
      }
      if (item.kind === 'view') {
        openView(item.target);
        return;
      }
      if (item.kind === 'query') {
        // Below a settings page, the web row is a plain search: picking it also teaches
        // Enter to search this text from now on.
        await run(typedText(), items.value[0]?.kind === 'query');
        return;
      }
      if (item.kind === 'app' && item.appId) {
        await launch(item.appId);
        return;
      }
      if (item.kind === 'window' && item.window) {
        await switchTo(item.window);
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
      await run(item.query, false);
      return;
    }

    await run(typedText(), true);
  }

  // Lists never scroll: only the rows that fit are shown (see v-fit), and arrows stay
  // among them.
  const visibleCount = ref(Infinity);

  function chooseAt(index: number) {
    selected.value = index;
    void accept();
  }

  // Shift+Space on a file (or Preview in its actions) opens a look at it beside the results;
  // it follows the selection until closed. On a Mac it's Quick Look instead.
  const previewOpen = ref(false);
  const inputRowEl = ref<HTMLElement | null>(null);
  const statsEl = ref<HTMLElement | null>(null);
  const previewBounds = ref({ top: 0, bottom: 0 });
  const previewShown = computed(
    () => previewOpen.value && view.value === 'root' && selectedItem.value?.kind === 'file'
  );
  const previewStyle = computed(() => ({
    top: `${previewBounds.value.top}px`,
    bottom: `${previewBounds.value.bottom}px`,
  }));

  function measurePreview() {
    const bar = root.value;
    const input = inputRowEl.value;
    if (!bar || !input) return;
    const footer = statsEl.value;
    previewBounds.value = {
      top: input.offsetTop + input.offsetHeight,
      bottom: footer ? bar.clientHeight - footer.offsetTop : 0,
    };
  }

  function togglePreview() {
    const path = selectedItem.value?.kind === 'file' ? selectedItem.value.path : undefined;
    if (!path) return;
    if (isMac) {
      void quickLook(path).catch((error) => (notice.value = errorMessage(error)));
      return;
    }
    previewOpen.value = !previewOpen.value;
    if (previewOpen.value) void nextTick(measurePreview);
  }

  /** A file result dragged out of the bar drops into the other app as that file. */
  function dragRow(index: number) {
    const row = items.value[index];
    if (row?.kind !== 'file' || !row.path) return;
    void dragOut({ path: row.path }).catch((error) => (notice.value = errorMessage(error)));
  }

  /** Clears the text, scope, selection and message once something has opened. */
  function clearAfterOpen() {
    query.value = '';
    scope.value = null;
    selected.value = -1;
    notice.value = null;
  }

  /** Waits on an open: the bar clears when it worked, and says why when it didn't. */
  async function opening(request: Promise<unknown>) {
    try {
      await request;
      clearAfterOpen();
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  async function switchTo(handle: number) {
    await opening(windowFocus(handle));
  }

  /** Closing asks the window, so the app can still offer to save; the list catches up. */
  async function closeWindow(handle: number) {
    try {
      await windowClose(handle);
      notice.value = 'Asked the window to close';
      later(scheduleSuggest, 400);
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  async function quitApp(appId: string, name: string) {
    try {
      await appQuit(appId);
      notice.value = `Asked ${name} to quit`;
      later(scheduleSuggest, 400);
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  /**
   * Opens an app; a running one is switched to unless `newWindow` asks for another, and
   * `privateWindow` opens a browser in its private window.
   */
  async function launch(appId: string, newWindow = false, privateWindow = false) {
    await opening(launchApp(appId, newWindow, privateWindow));
  }

  async function openPage(settingId: string) {
    await opening(openSetting(settingId));
  }

  async function openPath(path: string) {
    await opening(openFile(path));
  }

  async function reveal(path: string) {
    await opening(revealFile(path));
  }

  async function copyPath(path: string) {
    try {
      await navigator.clipboard.writeText(path);
      notice.value = 'Copied the path';
    } catch {
      notice.value = "Couldn't copy the path";
    }
  }

  /** Does what Enter would with `text`; `interpret` off searches it as it is. */
  async function run(text: string, interpret: boolean) {
    try {
      const outcome = await dispatch(text, interpret);
      if (outcome.kind !== 'empty') {
        clearAfterOpen();
        return;
      }
      notice.value = null;
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }
</script>

<template>
  <div
    ref="root"
    class="bar"
    :class="{ moving: movable, previewing: previewShown }"
    role="dialog"
    aria-label="Zephyr"
    @mousedown.capture="dragWithCtrl"
    @contextmenu.prevent
  >
    <span
      v-if="capturing"
      class="capture-dot"
      v-tip="`Recording · ${isMac ? '⌃R' : 'Ctrl+R'} stops`"
    />
    <DeviceCard v-if="devicesWaiting.length" :device="devicesWaiting[0]" @answer="answerDevice" />
    <ShellView
      v-if="visited.shell"
      v-show="shellMode"
      ref="shellView"
      :initial-query="shellQuery"
      :settings="snapshot?.shell ?? null"
      @exit="leave"
    />
    <TypingView
      v-if="visited.typing"
      v-show="typingMode"
      ref="typingView"
      :bests="snapshot?.typingBests ?? []"
      @exit="leave"
    />
    <ResourcesView
      v-if="view === 'resources'"
      ref="resourcesView"
      @exit="leave"
      @search="searchFromResources"
      @network="openView('network')"
      :initial-filter="resourcesFilter"
    />
    <NetworkView
      v-if="view === 'network'"
      ref="networkView"
      :last-speed="snapshot?.lastSpeed ?? null"
      @exit="leave"
    />
    <ClipboardView
      v-if="clipMode"
      ref="clipView"
      :initial-query="clipQuery"
      @exit="leave"
      @picked="leave"
    />
    <template v-if="view === 'root'">
      <div ref="inputRowEl" class="input-row">
        <svg class="mark" viewBox="0 0 24 24" aria-hidden="true">
          <circle cx="11" cy="11" r="6.5" />
          <path d="M16 16.5 20 20.5" />
        </svg>
        <span v-if="scope" class="chip scope-label">{{ scope.name }}</span>
        <div class="query-wrap">
          <!-- The typed text is invisible here; it only pushes the completion into place. -->
          <span v-if="ghost" class="query ghost" aria-hidden="true"
            ><span class="ghost-typed">{{ query }}</span
            >{{ ghost }}</span
          >
          <input
            ref="inputEl"
            v-model="query"
            class="query"
            type="text"
            role="combobox"
            aria-autocomplete="both"
            :aria-controls="actionsOpen ? 'actions' : 'results'"
            :aria-expanded="items.length > 0"
            :aria-activedescendant="
              actionsOpen
                ? `actions-${actionIndex}`
                : selected >= 0
                  ? `result-${selected}`
                  : undefined
            "
            :placeholder="scope ? `Search ${scope.name.toLowerCase()}` : 'Search or open an app'"
            spellcheck="false"
            autocomplete="off"
            autocapitalize="off"
            @input="trackCaret"
            @keyup="trackCaret"
            @click="trackCaret"
            @select="trackCaret"
          />
        </div>
        <!-- What's playing, at the end of the field, in every view of the main bar. -->
        <NowPlayingStrip
          v-if="playingNow"
          :track="playingNow"
          @toggle="media.press('toggle')"
          @next="media.press('next')"
          @previous="media.press('previous')"
          @focus="media.focus()"
        />
        <!-- With Ctrl held the bar can be dragged. -->
        <span v-if="movable" class="move-hint">
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path d="M12 3v18M3 12h18M12 3 9.5 5.5M12 3l2.5 2.5M12 21l-2.5-2.5M12 21l2.5-2.5" />
            <path d="M3 12l2.5-2.5M3 12l2.5 2.5M21 12l-2.5-2.5M21 12l-2.5 2.5" />
          </svg>
          Drag to move
        </span>
        <button
          v-if="updateReady"
          v-tip="`Zephyr ${updateReady} is ready\nClick to restart and update`"
          type="button"
          class="bar-update"
          @mousedown.prevent
          @click="installUpdate"
        >
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path d="M12 16V5M7.5 9.5 12 5l4.5 4.5M6 19h12" />
          </svg>
          Update
        </button>
        <button
          type="button"
          class="bar-settings"
          v-tip="`Zephyr settings (${isMac ? '⌘S' : 'Ctrl+S'})`"
          aria-label="Zephyr settings"
          @mousedown.prevent
          @click="openSettings"
        >
          <svg viewBox="0 0 24 24" aria-hidden="true" v-html="GLYPHS.gear" />
        </button>
      </div>

      <p v-if="loadError || notice" class="notice">{{ loadError || notice }}</p>

      <div class="bar-body">
        <WeatherCard v-if="cardForecast" :forecast="cardForecast" @open="openForecast" />
        <TickerCard
          v-else-if="cardQuote"
          :quote="cardQuote"
          :range="quoteRange"
          @open="openQuote"
          @range="pickRange"
        />
        <ActionMenu
          v-if="actionsOpen"
          id="actions"
          :actions="actions"
          :index="actionIndex"
          @run="menu.run"
          @hover="menu.hover"
          @fit="menu.shown.value = $event"
        />

        <ResultList
          v-else-if="items.length"
          :groups="sections"
          :selected="selected"
          :numbered="showNumbers ? RUN_KEY : ''"
          @choose="chooseAt"
          @hover="hover"
          @drag="dragRow"
          @fit="visibleCount = $event"
        >
          <template #card="{ row, selected: chosen }">
            <PortCard
              v-if="row.kind === 'portCard'"
              :row="row"
              :selected="chosen"
              :confirm="portConfirmText"
            />
            <NetworkCard
              v-else
              :row="row"
              :selected="chosen"
              :summary="netSummary"
              :last-speed="snapshot?.lastSpeed ?? null"
            />
          </template>
        </ResultList>
      </div>

      <!-- After the list's v-if chain, which it must not join. -->
      <FilePreviewPane
        v-if="previewShown && selectedItem?.path"
        :path="selectedItem.path"
        :name="selectedItem.label"
        :icon="selectedItem.icon"
        :style="previewStyle"
      />

      <!-- The meters at the left (a click opens System Resources); the weather at the right. -->
      <footer
        v-if="(statsShown && meters) || forecast"
        ref="statsEl"
        class="stats"
        aria-label="System"
        @mousedown.prevent
      >
        <span
          v-if="statsShown && meters"
          class="meters"
          role="button"
          aria-label="Open System Resources"
          @click="openView('resources')"
        >
          <span v-tip="`CPU ${meters.cpu}\nClick for System Resources`" class="stat">
            <svg viewBox="0 0 24 24" aria-hidden="true" v-html="GLYPHS.cpu" />
            <span class="meter" aria-hidden="true">
              <span
                class="meter-fill"
                :class="{ hot: meters.cpuHot }"
                :style="{ transform: `scaleX(${meters.cpuScale})` }"
              />
            </span>
            <span class="stat-value">{{ meters.cpu }}</span>
          </span>
          <span
            v-if="meters.gpu"
            v-tip="`${meters.gpuTitle}\nClick for System Resources`"
            class="stat"
          >
            <svg viewBox="0 0 24 24" aria-hidden="true" v-html="GLYPHS.gpu" />
            <span class="meter" aria-hidden="true">
              <span
                class="meter-fill"
                :class="{ hot: meters.gpuHot }"
                :style="{ transform: `scaleX(${meters.gpuScale})` }"
              />
            </span>
            <span class="stat-value">{{ meters.gpu }}</span>
          </span>
          <span v-tip="`${meters.memoryTitle}\nClick for System Resources`" class="stat">
            <svg viewBox="0 0 24 24" aria-hidden="true" v-html="GLYPHS.memory" />
            <span class="meter" aria-hidden="true">
              <span class="meter-fill" :style="{ transform: `scaleX(${meters.memoryScale})` }" />
            </span>
            <span class="stat-value">{{ meters.memory }}</span>
          </span>
          <span v-if="meters.diskLow" v-tip="meters.diskTitle" class="stat stat-low">
            <svg viewBox="0 0 24 24" aria-hidden="true" v-html="GLYPHS.drive" />
            {{ meters.disk }}
          </span>
        </span>
        <span
          v-if="netAlert"
          v-tip="
            netAlert.kind === 'offline'
              ? 'No route to the internet\nClick for Network'
              : netAlert.kind === 'drop'
                ? 'The internet stopped answering\nClick for Network'
                : 'Much slower than usual\nClick for Network'
          "
          class="net-alert"
          :class="netAlert.kind"
          role="button"
          aria-label="Open Network"
          @click="openView('network')"
        >
          <svg viewBox="0 0 24 24" aria-hidden="true" v-html="GLYPHS.updown" />
          {{ netAlertText }}
        </span>
        <FooterWeather v-if="forecast" :forecast="forecast" @open="showWeather" />
      </footer>
    </template>
  </div>
</template>
