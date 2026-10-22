<script setup lang="ts">
  import { computed, nextTick, onMounted, onUnmounted, reactive, ref, watch } from 'vue';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import {
    claudeAnswer,
    claudeApprovals,
    claudeJobs,
    dispatch,
    errorMessage,
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
    claudeSubmit,
    syncApprove,
    syncPending,
    syncRevoke,
    openNotes,
    openSettings,
    setActiveMode,
    resolveUrl,
    revealFile,
    setBarHeight,
    suggest,
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
    powerPreview,
    powerRun,
    powerCancel,
    openUrl,
  } from '../api';
  import ClipboardView from './ClipboardView.vue';
  import ResourcesView from './ResourcesView.vue';
  import TypingView from '../typing/TypingView.vue';
  import AiView from '../ai/AiView.vue';
  import ApprovalCard from '../claude/ApprovalCard.vue';
  import DeviceCard from './DeviceCard.vue';
  import ResultList from './ResultList.vue';
  import { vFit } from '../fit';
  import { bytes } from '../format';
  import { askAiName, chosenAi, displayName, isOtherAi } from '../aiChoice';
  import NowPlayingStrip from './NowPlayingStrip.vue';
  import { dragOut } from './dragOut';
  import { GLYPHS } from './glyphs';
  import FilePreviewPane from './FilePreviewPane.vue';
  import { useNowPlaying } from './nowPlaying';
  import { bindingForAlias, bindingTarget, buildRows, groups, rowKey, type RowCtx } from './rows';
  import ClaudeView from '../claude/ClaudeView.vue';
  import ShellView from '../shell/ShellView.vue';
  import {
    isSnapshot,
    type Binding,
    type ClaudeApproval,
    type ClaudeJob,
    type SyncPending,
    type Destination,
    type Snapshot,
    type Suggestion,
    type SystemStats,
    type SuggestResponse,
    type PowerPreview,
  } from '../types';

  const root = ref<HTMLElement | null>(null);
  const inputEl = ref<HTMLInputElement | null>(null);
  const snapshot = ref<Snapshot | null>(null);
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
  const armedId = ref('google');
  // The last suggest reply and the text it was for; the rows are built from it, so command
  // rows and job counts stay current without asking again.
  const lastResponse = ref<SuggestResponse | null>(null);
  const lastQuery = ref('');
  const mode = ref<'search' | 'destinations' | 'recent'>('recent');
  const notice = ref<string | null>(null);
  const selected = ref(-1);
  const loadError = ref<string | null>(null);

  // Views that take over the bar. Workspaces (Claude, Ask AI, typing, and Notes in its own
  // window) stick: every open lands back in them until Esc leaves. Esc to the main bar only
  // parks a workspace; a second Esc there clears it. Clipboard is a picker: it sticks until
  // something is picked.
  type View = 'root' | 'claude' | 'clip' | 'typing' | 'ai' | 'shell' | 'resources';
  const WORKSPACES = ['claude', 'ai', 'typing', 'shell'];
  const MODE_NAMES: Record<string, string> = {
    claude: 'Claude jobs',
    ai: 'Ask AI',
    typing: 'typing test',
    shell: 'Shell',
  };
  const view = ref<View>('root');
  const parked = ref('');
  // A view is mounted on first use and then kept, so its state survives leaving it.
  const visited = reactive<Record<string, boolean>>({});
  const clipMode = computed(() => view.value === 'clip');
  const typingMode = computed(() => view.value === 'typing');
  const claudeMode = computed(() => view.value === 'claude');
  const aiMode = computed(() => view.value === 'ai');
  const shellMode = computed(() => view.value === 'shell');
  // Shell commands, from > or !sh; a command never runs on unscoped Enter.
  const shellQuery = ref('');
  const shellView = ref<InstanceType<typeof ShellView> | null>(null);
  const SHELL_PREFIX = /^\s*(?:>|!(?:sh|shell)(?:\s|$))\s*/i;
  const clipQuery = ref('');
  const resourcesView = ref<InstanceType<typeof ResourcesView> | null>(null);
  const clipView = ref<InstanceType<typeof ClipboardView> | null>(null);
  const typingView = ref<InstanceType<typeof TypingView> | null>(null);
  const aiView = ref<InstanceType<typeof AiView> | null>(null);
  // Background Claude jobs, from !claude; pending permission requests show above everything.
  const claudeQuery = ref('');
  const claudeView = ref<InstanceType<typeof ClaudeView> | null>(null);
  const approvals = ref<ClaudeApproval[]>([]);
  const jobs = ref<ClaudeJob[]>([]);
  // Other computers asking to join sync; checked when the bar opens and every minute.
  const devicesWaiting = ref<SyncPending[]>([]);
  let devicePoll = 0;

  // Reopening soon after closing returns to the view and text it closed on.
  let hiddenAt = 0;

  interface Action {
    label: string;
    hint?: string;
    run: () => unknown;
  }

  const actionsOpen = ref(false);
  const actionIndex = ref(0);

  let generation = 0;
  let remoteApplied = false;
  let userMoved = false;
  // Arrow presses that land before the first rows do.
  let pendingMove = 0;
  let suggestTimer = 0;
  let unlistens: UnlistenFn[] = [];
  let observer: ResizeObserver | null = null;

  const strip = computed(() =>
    (snapshot.value?.destinations ?? [])
      .filter(
        (destination) =>
          destination.pinned && !destination.disabled && !isOtherAi(snapshot.value, destination)
      )
      .slice(0, 8)
  );

  const destinationNames = computed(() =>
    Object.fromEntries(
      (snapshot.value?.destinations ?? []).map((item) => [
        item.id,
        displayName(snapshot.value, item),
      ])
    )
  );

  const armed = computed(() =>
    snapshot.value?.destinations.find((destination) => destination.id === armedId.value)
  );

  const rowCtx = computed<RowCtx>(() => ({
    snapshot: snapshot.value,
    armedId: armedId.value,
    jobs: jobs.value,
    parked: parked.value,
    parkedName: MODE_NAMES[parked.value] ?? '',
    strip: strip.value.map((destination) => destination.id),
    typed: scope.value ? '' : lastQuery.value,
  }));

  // Power commands ("shutdown in 30m"): what the typed text would do, a pending schedule with
  // its countdown, and the first Enter of a shutdown, restart or sign-out waiting for its
  // second.
  const powerRow = ref<PowerPreview | null>(null);
  const confirmArm = ref('');
  const clock = ref(Date.now());
  window.setInterval(() => (clock.value = Date.now()), 15000);

  function untilText(at: number): string {
    const left = Math.max(0, at - Math.floor(clock.value / 1000));
    const hours = Math.floor(left / 3600);
    const minutes = Math.ceil((left % 3600) / 60);
    const span = hours ? `${hours}h${minutes ? ` ${minutes}m` : ''}` : `${minutes || 1}m`;
    if (left < 3600) return `in ${span}`;
    const time = new Date(at * 1000).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
    return `at ${time}, in ${span}`;
  }

  const POWER_ICONS: Record<string, string> = {
    shutdown: 'glyph:power',
    restart: 'glyph:restart',
    sleep: 'glyph:moon',
    lock: 'glyph:lock',
    sign_out: 'glyph:signout',
    firmware: 'glyph:chip',
  };
  const POWER_LABELS: Record<string, string> = {
    shutdown: 'Shut down',
    restart: 'Restart',
    sleep: 'Sleep',
    lock: 'Lock',
    sign_out: 'Sign out',
    firmware: 'Restart to BIOS',
  };
  const POWER_NAMES: Record<string, string> = {
    shutdown: 'Shutting down',
    restart: 'Restarting',
    sleep: 'Sleeping',
    lock: 'Locking',
    sign_out: 'Signing out',
    firmware: 'Restarting to BIOS',
  };

  function powerRows(): Suggestion[] {
    const rows: Suggestion[] = [];
    const pending = snapshot.value?.power;
    if (pending) {
      rows.push({
        kind: 'powerPending',
        label: `${POWER_NAMES[pending.action] ?? 'Scheduled'} ${untilText(pending.at)}`,
        query: '',
        destinationId: '',
        hint: 'Enter cancels',
        section: 'power',
        icon: POWER_ICONS[pending.action] ?? 'glyph:power',
      });
    }
    const preview = powerRow.value;
    if (preview && !(preview.cancel && !pending)) {
      const key = `${preview.action}:${preview.at ?? 'now'}`;
      rows.push({
        kind: 'power',
        label: preview.label,
        query: '',
        destinationId: '',
        hint:
          confirmArm.value === key
            ? 'Enter again to confirm'
            : preview.cancel
              ? 'Cancel'
              : preview.at
                ? 'Schedule'
                : 'Run',
        section: 'power',
        icon: POWER_ICONS[preview.action] ?? 'glyph:power',
        power: preview,
      });
      // Under a plain "shutdown", a few timers, so it's clear they exist. They're cancelable
      // from the top of the bar, so they skip the second Enter.
      if (!preview.at && !preview.cancel && preview.action !== 'lock') {
        const now = Math.floor(Date.now() / 1000);
        for (const [seconds, words] of [
          [300, '5 minutes'],
          [900, '15 minutes'],
          [3600, '1 hour'],
        ] as const) {
          rows.push({
            kind: 'power',
            label: `${POWER_LABELS[preview.action]} in ${words}`,
            query: '',
            destinationId: '',
            hint: 'Schedule',
            section: 'power',
            icon: 'glyph:clock',
            power: {
              ...preview,
              label: `${POWER_LABELS[preview.action]} in ${words}`,
              at: now + seconds,
              confirm: false,
              exact: false,
            },
          });
        }
      }
    }
    return rows;
  }

  const built = computed(() => {
    if (!lastResponse.value) return { rows: [] as Suggestion[], preselect: null };
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
        destinationId: 'shell',
        kind: 'view',
        section: 'shell',
        icon: 'glyph:prompt',
      });
    }
    return result;
  });
  const items = computed(() => built.value.rows);
  const sections = computed(() => groups(items.value, rowCtx.value));

  const selectedItem = computed(() =>
    selected.value >= 0 ? items.value[selected.value] : undefined
  );

  // Type-ahead: the rest of what Enter would act on, drawn faintly after the cursor. Enter
  // still sends only what was typed; Right arrow or End takes the completion.
  const caretAtEnd = ref(true);
  const ghost = computed(() => {
    const typed = query.value;
    if (!typed.trim() || !caretAtEnd.value || mode.value === 'destinations') return '';
    if (scope.value || typed.startsWith('!') || typed.startsWith('>')) return '';
    const lower = typed.toLowerCase();
    const target = selectedItem.value;
    let candidates: string[];
    if (target?.kind === 'app') {
      candidates = [target.label];
    } else if (target?.kind === 'history' || target?.kind === 'remote') {
      candidates = [target.query];
    } else {
      const history = [...(snapshot.value?.history ?? [])]
        .filter((entry) => entry.destinationId === armedId.value)
        .sort((a, b) => b.uses - a.uses || b.lastUsed - a.lastUsed)
        .map((entry) => entry.query);
      const remote = items.value
        .filter((item) => item.kind === 'history' || item.kind === 'remote')
        .map((item) => item.query);
      candidates = [...history, ...remote];
    }
    const match = candidates.find(
      (candidate) => candidate.length > typed.length && candidate.toLowerCase().startsWith(lower)
    );
    return match ? match.slice(typed.length) : '';
  });

  /** The completion only shows while the cursor sits at the end of text that fits the box. */
  function trackCaret() {
    const input = inputEl.value;
    if (!input) return;
    const end = input.value.length;
    caretAtEnd.value =
      input.selectionStart === end &&
      input.selectionEnd === end &&
      input.scrollWidth <= input.clientWidth;
  }

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

  // Bug captures (Ctrl+Alt+R and Ctrl+Alt+S, unless turned off in Settings): while one runs,
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
        armed: armed.value?.name ?? null,
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

  const isMac = navigator.userAgent.includes('Mac');

  // Ctrl+K (⌘K on a Mac) lists what else the selected row, or the typed text, can do.
  const rowActions = computed<Action[]>(() => {
    const item = selectedItem.value;
    const list: Action[] = [];
    if (item?.kind === 'answer') {
      list.push({ label: 'Copy answer', run: () => copyText(item.label) });
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
          hint: '⇧↵',
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
          hint: '⇧↵',
          run: () => launch(appId, false, true),
        });
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
    } else if (item?.kind === 'destination') {
      const destinationId = item.destinationId;
      list.push({ label: `Use ${item.label}`, run: () => armById(destinationId) });
      list.push({ label: 'Edit destinations', run: () => openPage('zephyr.destinations') });
    } else {
      const text = item ? item.query : query.value.trim();
      const target = item ? item.destinationId : armedId.value;
      if (!text) return list;
      for (const destination of sendTargets(target)) {
        list.push({
          label:
            destination.id === target
              ? `Search ${destination.name}`
              : `Send to ${destination.name}`,
          run: () => run(text, destination.id, false),
        });
      }
      const current = snapshot.value?.destinations.find((destination) => destination.id === target);
      if (current && current.kind !== 'ai') {
        list.push({ label: 'Copy link', run: () => copyLink(text, target) });
      }
      list.push({ label: 'Copy text', run: () => copyText(text) });
    }
    return list;
  });

  // Anything that can have a hotkey or alias gets one more action, which opens Settings on it.
  const actions = computed<Action[]>(() => {
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

  // CPU, memory, disk and network in a slim footer, sampled once a second only while the
  // bar is showing (Settings › General can turn it off).
  const stats = ref<SystemStats | null>(null);
  let statsTimer = 0;
  /** Bumped by every start and stop, so a sample still in flight can't reschedule. */
  let statsGeneration = 0;
  const statsShown = computed(() => snapshot.value?.showStats !== false && stats.value !== null);

  /** Samples while the main bar shows; other views don't show the footer. */
  function sampleStats() {
    stopStats();
    if (snapshot.value?.showStats === false || view.value !== 'root') return;
    const current = statsGeneration;
    void systemStats()
      .then((next) => {
        if (current === statsGeneration) stats.value = next;
      })
      .catch(() => undefined)
      .finally(() => {
        if (current === statsGeneration) statsTimer = window.setTimeout(sampleStats, 1000);
      });
  }

  function stopStats() {
    statsGeneration += 1;
    window.clearTimeout(statsTimer);
    statsTimer = 0;
  }

  // What's playing, as a small chip at the end of the search bar.
  const media = useNowPlaying();
  const playingNow = computed(() =>
    snapshot.value?.showNowPlaying === false ? null : media.shown.value
  );

  onMounted(async () => {
    try {
      snapshot.value = await getSnapshot();
      armedId.value = snapshot.value.defaultDestinationId;
      const remembered = snapshot.value.activeMode;
      if (snapshot.value.returnToMode && MODE_NAMES[remembered]) {
        parked.value = remembered;
        if (WORKSPACES.includes(remembered)) enter(remembered as View);
      }
    } catch (error) {
      loadError.value = errorMessage(error);
    }

    unlistens.push(
      await listen('bar-shown', () => {
        media.tick();
        sampleStats();
        // Opens the connection to the suggestion service before the first keystroke.
        void warmSuggestions(armedId.value).catch(() => undefined);
        void loadApprovals();
        void loadJobs();
        void loadDevices();
        if (snapshot.value && !snapshot.value.returnToMode) {
          parked.value = '';
          view.value = 'root';
          resetForSummon();
          scheduleSuggest();
          void focusInput();
          return;
        }
        // Still inside a view (a workspace, or a picker mid-search): reopen right there.
        if (view.value === 'resources') resourcesView.value?.start();
        if (view.value !== 'root') return;
        if (WORKSPACES.includes(parked.value)) {
          enter(parked.value as View);
          return;
        }
        // The main bar keeps its text only briefly.
        const limit = (snapshot.value?.resumeSeconds ?? 120) * 1000;
        const recent = hiddenAt > 0 && limit > 0 && Date.now() - hiddenAt < limit;
        // A scoped search (files, apps, settings, notes) sticks until something is picked.
        const picker = scope.value !== null || /^!(note|notes)\b/i.test(query.value);
        if (!recent && !picker) resetForSummon();
        // The empty bar's list changes while hidden (apps found after startup, a launch, a
        // search), and the text is unchanged, so nothing else would refresh it.
        if (!query.value) scheduleSuggest();
        void focusInput();
      })
    );
    unlistens.push(
      // Startup's unseen draw, again once the apps are found, so the first summon has them.
      await listen('bar-redraw', () => {
        if (!query.value) scheduleSuggest();
      })
    );
    unlistens.push(
      await listen('bar-hidden', () => {
        hiddenAt = Date.now();
        stopStats();
        resourcesView.value?.stop();
      })
    );
    // A zephyr:// link: text to type in, and optionally where to send it right away.
    unlistens.push(
      await listen<{ query: string; destination: string | null; run: boolean }>(
        'bar-input',
        (event) => {
          const { query: text, destination, run: send } = event.payload;
          // A link from a web page must never stage a shell command for the user to run.
          if (SHELL_PREFIX.test(text)) {
            notice.value = "Links can't open the shell";
            return;
          }
          const target = snapshot.value?.destinations.find(
            (item) =>
              !item.disabled &&
              (item.id === destination || item.triggers.includes(destination ?? ''))
          );
          if (target) armedId.value = target.id;
          query.value = text;
          if (send && text) {
            void run(text, target?.id ?? armedId.value, !target);
          } else if (destination && !target) {
            notice.value = `No destination ${destination}`;
          }
          void focusInput();
        }
      )
    );
    unlistens.push(
      await listen('claude-changed', () => {
        void loadApprovals();
        void loadJobs();
      })
    );
    void loadApprovals();
    void loadJobs();
    void loadDevices();
    devicePoll = window.setInterval(() => void loadDevices(), 60000);
    unlistens.push(
      await listen('clipboard-shown', () => {
        clipQuery.value = '';
        enter('clip');
      })
    );
    // A hotkey the user gave a mode or scoped search (Settings > Shortcuts).
    unlistens.push(
      await listen<string>('bar-open', (event) => {
        query.value = '';
        openView(event.payload);
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
    window.addEventListener('keydown', trackModifiers, true);
    window.addEventListener('keyup', trackModifiers, true);
    window.addEventListener('blur', () => {
      held.value = '';
      movable.value = false;
    });
    // A drop lands outside the page's key events, so the pointer corrects what's held.
    window.addEventListener('mousemove', trackModifiers, true);
    if (root.value) {
      observer = new ResizeObserver(() => syncHeight());
      observer.observe(root.value);
      syncHeight();
    }
    // One sample now, so the footer is already there the first time the bar opens instead of
    // popping in a second later.
    if (snapshot.value?.showStats !== false) {
      void systemStats()
        .then((next) => (stats.value ??= next))
        .catch(() => undefined);
    }
    await focusInput();
    scheduleSuggest();
    // Drawn with its content, the bar goes back off screen until it's summoned (window.rs).
    pageReady();
  });

  onUnmounted(() => {
    window.clearInterval(devicePoll);
    window.removeEventListener('keydown', onKey, true);
    window.removeEventListener('keydown', trackModifiers, true);
    window.removeEventListener('keyup', trackModifiers, true);
    window.removeEventListener('mousemove', trackModifiers, true);
    observer?.disconnect();
    window.clearTimeout(suggestTimer);
    for (const unlisten of unlistens) unlisten();
  });

  watch(selected, () => {
    actionsOpen.value = false;
  });

  watch(view, async () => {
    await nextTick();
    syncHeight();
    if (view.value === 'root') {
      scheduleSuggest();
      sampleStats();
      void focusInput();
    } else {
      stopStats();
      if (view.value === 'ai') void aiView.value?.focus();
    }
  });

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

  /** Enters a view carrying text; a view kept from earlier takes the new text too. */
  function enterWith(next: 'claude' | 'shell', text: string) {
    if (next === 'claude') {
      claudeQuery.value = text;
      if (claudeView.value && text) claudeView.value.text = text;
    } else {
      shellQuery.value = text;
      if (shellView.value && text) shellView.value.text = text;
    }
    enter(next);
  }

  /** Esc or Backspace out of a view: back to the main bar, with a workspace left parked. */
  function leave() {
    view.value = 'root';
  }

  /** System Resources' "Search the web": the default web destination, from the main bar. */
  function searchFromResources(name: string) {
    leave();
    const destination = webDefault();
    if (destination) void run(name, destination, false);
  }

  /** System Resources' "Ask AI": the AI chosen in Settings › AI, built in or on the web. */
  function askFromResources(question: string) {
    const ai = chosenAi(snapshot.value);
    if (ai && ai.kind !== 'ai') {
      leave();
      void run(question, ai.id, false);
    } else {
      void enterAi(question);
    }
  }

  /** Esc on the main bar: forget the parked workspace, then close. */
  function closeFromRoot() {
    if (parked.value) {
      parked.value = '';
      void setActiveMode('').catch(() => undefined);
    }
    void hideBar();
  }

  async function enterAi(question: string, fresh = true) {
    enter('ai');
    await nextTick();
    await nextTick();
    void aiView.value?.ask(question, fresh);
  }

  // A pasted line that reads like a command gets a "Run in shell" row; Enter still goes to
  // the armed destination unless that row is picked.
  const COMMAND_START =
    /^(?:git|gh|ls|ll|cd|cat|echo|grep|rg|find|curl|wget|npm|npx|bun|bunx|pnpm|yarn|node|deno|python3?|py|pip3?|uv|cargo|rustup|go|make|docker|kubectl|ssh|scp|rsync|brew|winget|choco|scoop|sudo|chmod|chown|mkdir|rm|mv|cp|touch|tail|head|ps|kill|df|du|code|claude|wsl|pwsh|powershell|tar|unzip|ping|nslookup|ipconfig|ifconfig|[A-Z][a-z]+-[A-Z]\w+)$/;

  function looksLikeCommand(text: string): boolean {
    const line = text.trim().replace(/^\$\s+/, '');
    if (!line || line.startsWith('!')) return false;
    if (/^(?:\.{1,2}|~)\//.test(line)) return true;
    const [first, ...rest] = line.split(/\s+/);
    return rest.length > 0 && (COMMAND_START.test(first) || /\s(?:\||&&)\s/.test(line));
  }

  async function loadJobs() {
    try {
      jobs.value = await claudeJobs();
    } catch {
      jobs.value = [];
    }
  }

  function openView(id: string) {
    const typed = query.value;
    query.value = '';
    const back = id === 'back';
    if (back) id = parked.value;
    if (id === 'shell') {
      enterWith('shell', back ? '' : typed.trim().replace(/^\$\s+/, ''));
    } else if (id === 'claude' || id === 'ai' || id === 'typing') {
      enter(id);
    } else if (id === 'clip') {
      clipQuery.value = '';
      enter('clip');
    } else if (id === 'resources') {
      enter('resources');
    } else if (id === 'notes') {
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

  async function loadDevices() {
    try {
      devicesWaiting.value = await syncPending();
    } catch {
      devicesWaiting.value = [];
    }
  }

  async function answerDevice(decision: 'allow' | 'deny') {
    const device = devicesWaiting.value[0];
    if (!device) return;
    try {
      if (decision === 'allow') await syncApprove(device.id);
      else await syncRevoke(device.id);
      await loadDevices();
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  async function loadApprovals() {
    try {
      approvals.value = await claudeApprovals();
    } catch {
      approvals.value = [];
    }
  }

  /** Answers the oldest permission request; text typed in the bar becomes a deny reason. */
  async function answerApproval(decision: 'allow' | 'always' | 'deny') {
    const approval = approvals.value[0];
    if (!approval) return;
    const typed = view.value === 'claude' ? (claudeView.value?.text ?? '') : query.value;
    try {
      await claudeAnswer(approval.id, decision, decision === 'deny' ? typed : undefined);
      if (decision === 'deny' && typed) {
        if (view.value === 'claude' && claudeView.value) claudeView.value.text = '';
        else query.value = '';
      }
      await loadApprovals();
    } catch (error) {
      notice.value = errorMessage(error);
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
      enterWith('shell', text);
      return;
    }
    const claude = /^!claude(?:\s+(.*))?$/i.exec(query.value);
    if (claude) {
      query.value = '';
      enterWith('claude', claude[1] ?? '');
      return;
    }
    const clip = /^!clip(?:\s+(.*))?$/i.exec(query.value);
    if (clip) {
      clipQuery.value = clip[1] ?? '';
      query.value = '';
      enter('clip');
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

  watch(armedId, (id) => {
    selected.value = -1;
    userMoved = false;
    scheduleSuggest();
    void warmSuggestions(id).catch(() => undefined);
  });

  /** Clears the main bar's text and selection; views and the parked workspace are untouched. */
  function resetForSummon() {
    query.value = '';
    scope.value = null;
    selected.value = -1;
    notice.value = null;
    armedId.value = snapshot.value?.defaultDestinationId ?? armedId.value;
  }

  async function focusInput() {
    await nextTick();
    inputEl.value?.focus();
    inputEl.value?.select();
  }

  // Every view is the bar's one fixed size, so the window (and its blur) always matches the
  // panel exactly. Views used to widen the window without widening the panel.
  function syncHeight() {
    if (!root.value) return;
    const rect = root.value.getBoundingClientRect();
    const height = Math.ceil(rect.height);
    trace('size', { height, width: Math.ceil(rect.width) });
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

  /** Web suggestions by destination and text, so going back to a text shows them at once. */
  const remembered = new Map<string, Suggestion[]>();

  /** The web rows to keep up until this text's own arrive: only ones that still match. */
  function stillMatching(typed: string, destinationId: string): Suggestion[] {
    const prefix = typed.trim().toLowerCase();
    if (!prefix) return [];
    return (lastResponse.value?.items ?? []).filter(
      (item) =>
        item.kind === 'remote' &&
        item.destinationId === destinationId &&
        item.query.toLowerCase().startsWith(prefix)
    );
  }

  async function loadSuggestions(current: number, includeRemote: boolean) {
    try {
      const typed = typedText();
      const destinationId = armedId.value;
      const asked = performance.now();
      trace('fetch', { text: typed, remote: includeRemote, generation: current });
      const [answer, power] = await Promise.all([
        suggest(typed, destinationId, includeRemote),
        includeRemote ? Promise.resolve(powerRow.value) : powerPreview(typed).catch(() => null),
      ]);
      let response = answer;
      if (!includeRemote && current === generation) powerRow.value = power;
      const stale = current !== generation || (!includeRemote && remoteApplied);
      trace('fetched', {
        text: typed,
        remote: includeRemote,
        ms: Math.round(performance.now() - asked),
        rows: response.items.length,
        stale,
      });
      if (stale) return;
      if (includeRemote) remoteApplied = true;
      // Until this text's web suggestions arrive, keep the ones seen for it before
      // (backspacing), or the current ones that still match, so the list doesn't collapse.
      const key = `${destinationId}\n${typed.trim().toLowerCase()}`;
      if (includeRemote) {
        remembered.set(
          key,
          response.items.filter((item) => item.kind === 'remote')
        );
        if (remembered.size > 200) remembered.delete(remembered.keys().next().value ?? '');
      } else {
        const carried = remembered.get(key) ?? stillMatching(typed, destinationId);
        if (carried.length) response = { ...response, items: [...response.items, ...carried] };
      }
      // Keep the row the user moved to selected when late results reshuffle the list.
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
    } catch (error) {
      if (current !== generation) return;
      notice.value = errorMessage(error);
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
    if (event.isComposing) return;
    // A pending Claude permission request: never plain Enter, only these keys.
    // A computer asking to join sync, when no Claude request is ahead of it.
    if (
      !approvals.value.length &&
      devicesWaiting.value.length &&
      (event.metaKey || event.ctrlKey) &&
      !event.altKey &&
      !event.shiftKey
    ) {
      if (event.code === 'KeyY' || event.code === 'KeyN') {
        event.preventDefault();
        void answerDevice(event.code === 'KeyY' ? 'allow' : 'deny');
        return;
      }
    }
    if (approvals.value.length && (event.metaKey || event.ctrlKey) && !event.altKey) {
      if (event.code === 'KeyY') {
        event.preventDefault();
        void answerApproval(event.shiftKey ? 'always' : 'allow');
        return;
      }
      if (event.code === 'KeyN' && !event.shiftKey) {
        event.preventDefault();
        void answerApproval('deny');
        return;
      }
    }
    // What's playing: Ctrl+Space plays or pauses anywhere in the bar; with nothing typed,
    // Ctrl+Shift+→/← skip (with text they still select by word).
    if (playingNow.value && event.ctrlKey && !event.altKey && !event.metaKey) {
      if (event.code === 'Space' && !event.shiftKey) {
        event.preventDefault();
        if (!event.repeat) media.press('toggle');
        return;
      }
      if (
        event.shiftKey &&
        view.value === 'root' &&
        !query.value &&
        (event.key === 'ArrowRight' || event.key === 'ArrowLeft')
      ) {
        event.preventDefault();
        if (!event.repeat) media.press(event.key === 'ArrowRight' ? 'next' : 'previous');
        return;
      }
    }
    if (view.value === 'claude') {
      claudeView.value?.onKey(event);
      return;
    }
    if (view.value === 'clip') {
      clipView.value?.onKey(event);
      return;
    }
    if (view.value === 'resources') {
      resourcesView.value?.onKey(event);
      return;
    }
    if (view.value === 'typing') {
      typingView.value?.onKey(event);
      return;
    }
    if (view.value === 'shell') {
      shellView.value?.onKey(event);
      return;
    }
    if (view.value === 'ai') {
      aiView.value?.onKey(event);
      return;
    }
    if (event.key === 'Enter' && /^!(?:sh|shell)$/i.test(query.value.trim())) {
      event.preventDefault();
      query.value = '';
      enterWith('shell', '');
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
    if (modifier && !event.altKey && !event.shiftKey && event.code === 'KeyK') {
      event.preventDefault();
      if (actionsOpen.value) {
        actionsOpen.value = false;
      } else if (actions.value.length > 0) {
        actionIndex.value = 0;
        actionsOpen.value = true;
      }
      return;
    }

    if (actionsOpen.value) {
      if (event.key === 'Escape') {
        event.preventDefault();
        actionsOpen.value = false;
        return;
      }
      if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
        event.preventDefault();
        const count = Math.min(actions.value.length, actionsShown.value);
        actionIndex.value =
          (actionIndex.value + (event.key === 'ArrowDown' ? 1 : -1) + count) % count;
        return;
      }
      if (event.key === 'Enter') {
        event.preventDefault();
        if (!event.repeat) runAction(actions.value[actionIndex.value]);
        return;
      }
      // Anything else goes back to typing.
      actionsOpen.value = false;
    }

    // Right arrow or End at the end of the text takes the suggested completion.
    if (
      (event.key === 'ArrowRight' || event.key === 'End') &&
      ghost.value &&
      !event.shiftKey &&
      !modifier &&
      !event.altKey
    ) {
      event.preventDefault();
      query.value += ghost.value;
      void nextTick(() => {
        inputEl.value?.setSelectionRange(query.value.length, query.value.length);
        trackCaret();
      });
      return;
    }

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

    if (event.key === 'Escape') {
      event.preventDefault();
      closeFromRoot();
      return;
    }

    if (event.key === 'Tab') {
      event.preventDefault();
      cycleArmed(event.shiftKey);
      return;
    }

    // Alt+1..9 (⌘1..9 on a Mac) runs the Nth row, as Enter on it would.
    if (runKeyHeld(event) && /^Digit[1-9]$/.test(event.code)) {
      const index = Number(event.code.slice(5)) - 1;
      if (index < items.value.length) {
        event.preventDefault();
        showNumbers.value = false;
        if (!event.repeat) chooseAt(index);
      }
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
      if (modifier || event.altKey) {
        void route(event.altKey ? 'alt' : 'cmd');
        return;
      }
      void accept();
    }
  }

  /**
   * Shift+Enter on a search going to Google opens it in Google's AI Mode (udm=50) instead
   * of the usual results. Anything else ignores it.
   */
  async function googleAiMode() {
    const item = selectedItem.value;
    const searchRow =
      item && ['query', 'history', 'remote', 'searchWith'].includes(item.kind) ? item : null;
    const text = (searchRow ? searchRow.query : typedText()).trim();
    const destinationId = searchRow ? searchRow.destinationId : armedId.value;
    if (!text || destinationId !== 'google' || (item && !searchRow)) return;
    try {
      const url = new URL(await resolveUrl(text, 'google'));
      url.searchParams.set('udm', '50');
      await openUrl(url.toString());
      query.value = '';
      scope.value = null;
      selected.value = -1;
      notice.value = null;
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  // Enter routes: ⌘↵ (Ctrl+Enter) and ⌥↵ (Alt+Enter) send the typed text somewhere else
  // without arming anything. Settings > General can point either at any destination or
  // Claude project.
  interface Route {
    kind: 'ai' | 'web' | 'claude';
    id: string;
    label: string;
  }

  function webDefault(): string {
    const destinations = snapshot.value?.destinations ?? [];
    const preferred = destinations.find(
      (item) => item.id === snapshot.value?.defaultDestinationId && item.kind !== 'ai'
    );
    return (
      (preferred ?? destinations.find((item) => !item.disabled && item.kind !== 'ai'))?.id ??
      'google'
    );
  }

  function routeFor(which: 'cmd' | 'alt'): Route {
    const configured =
      (which === 'cmd' ? snapshot.value?.routeCmd : snapshot.value?.routeAlt) ?? '';
    const destinations = snapshot.value?.destinations ?? [];
    const projects = snapshot.value?.claude.projects ?? [];
    const claudeRoute = (projectId: string): Route => {
      const project =
        projects.find((item) => item.id === projectId) ??
        projects.find((item) => item.id === snapshot.value?.claude.lastProject) ??
        projects[0];
      const name = project ? project.alias || project.folder.split(/[\\/]/).pop() : 'Claude';
      return { kind: 'claude', id: project?.id ?? '', label: `Claude job in ${name}` };
    };
    if (configured.startsWith('claude')) return claudeRoute(configured.slice(7));
    const chosen = destinations.find((item) => item.id === configured && !item.disabled);
    if (chosen) {
      return chosen.kind === 'ai'
        ? { kind: 'ai', id: chosen.id, label: askAiName(snapshot.value) }
        : { kind: 'web', id: chosen.id, label: chosen.name };
    }
    if (which === 'alt') return claudeRoute('');
    // The other primary action: the chosen AI when a search is armed, and the reverse.
    const ai = chosenAi(snapshot.value);
    if (armed.value && ai && armed.value.id === ai.id) {
      const id = webDefault();
      return {
        kind: 'web',
        id,
        label: destinations.find((item) => item.id === id)?.name ?? 'search',
      };
    }
    if (ai && ai.kind !== 'ai') return { kind: 'web', id: ai.id, label: ai.name };
    return { kind: 'ai', id: 'ai', label: askAiName(snapshot.value) };
  }

  async function route(which: 'cmd' | 'alt') {
    // A row with its own alternate action keeps it for ⌘↵.
    const item = selectedItem.value;
    if (which === 'cmd' && item?.kind === 'app' && item.appId) {
      await launch(item.appId);
      return;
    }
    const text = query.value.trim();
    if (!text) return;
    const target = routeFor(which);
    if (target.kind === 'ai') {
      query.value = '';
      await enterAi(text);
    } else if (target.kind === 'claude') {
      try {
        await claudeSubmit(text, target.id || undefined);
        query.value = '';
      } catch (error) {
        notice.value = errorMessage(error);
      }
    } else {
      await run(text, target.id, false);
    }
  }

  // Holding ⌘ or ⌥ previews where Enter would go.
  const held = ref<'' | 'cmd' | 'alt'>('');
  // ⌘ alone on a Mac (Ctrl elsewhere) means "drag to move": the cursor and the search bar
  // say so while it's held.
  const movable = ref(false);
  let gridReady = false;
  function trackModifiers(event: KeyboardEvent | MouseEvent) {
    held.value = event.altKey ? 'alt' : event.metaKey || event.ctrlKey ? 'cmd' : '';
    movable.value = moveKeyOnly(event);
    // The grid's window is made the first time Ctrl is held, so the drag never waits on it.
    if (movable.value && !gridReady) {
      gridReady = true;
      void prepareGrid().catch(() => (gridReady = false));
    }
    trackRunKey(event);
  }

  // Alt+1..9 (⌘1..9 on a Mac) runs the Nth row. Holding the key a moment, or tapping Alt
  // alone on Windows, numbers the rows; any other key hides the numbers again.
  const RUN_KEY = isMac ? '⌘' : 'Alt+';
  const showNumbers = ref(false);
  let numbersTimer = 0;
  let altAlone = false;
  function runKeyHeld(event: KeyboardEvent | MouseEvent) {
    return isMac
      ? event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey
      : event.altKey && !event.ctrlKey && !event.metaKey && !event.shiftKey;
  }
  function trackRunKey(event: KeyboardEvent | MouseEvent) {
    const keyboard = event instanceof KeyboardEvent;
    if (keyboard && event.type === 'keydown') {
      altAlone = !isMac && event.key === 'Alt';
      if (event.key !== 'Alt' && event.key !== 'Meta') showNumbers.value = false;
    }
    if (keyboard && event.type === 'keyup' && event.key === 'Alt' && !isMac) {
      // A bare Alt would otherwise move the webview's focus to a (missing) menu bar.
      event.preventDefault();
      window.clearTimeout(numbersTimer);
      numbersTimer = 0;
      showNumbers.value = altAlone;
      altAlone = false;
      return;
    }
    if (runKeyHeld(event)) {
      if (!numbersTimer && !showNumbers.value) {
        numbersTimer = window.setTimeout(() => {
          numbersTimer = 0;
          showNumbers.value = true;
        }, 300);
      }
    } else if (!keyboard || event.type === 'keyup') {
      window.clearTimeout(numbersTimer);
      numbersTimer = 0;
      if (isMac || keyboard) showNumbers.value = false;
    }
  }

  function runAction(action: Action | undefined) {
    if (!action) return;
    actionsOpen.value = false;
    void action.run();
  }

  function sendTargets(firstId: string): Destination[] {
    const all = (snapshot.value?.destinations ?? []).filter((destination) => !destination.disabled);
    const first = all.filter((destination) => destination.id === firstId);
    return [...first, ...all.filter((destination) => destination.id !== firstId)].slice(0, 9);
  }

  function armById(destinationId: string) {
    armedId.value = destinationId;
    query.value = '';
    inputEl.value?.focus();
  }

  /**
   * Aliases the user set (Settings > Shortcuts). A direct one fires as soon as it's typed;
   * any other, typed and then a space, goes into its destination or command with whatever
   * follows. An app's or bookmark's alias waits for Enter (its row tops the list instead).
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
    if (!/^(dest|view|scope):/.test(binding.target)) return false;
    void runBinding(binding, spaced[2]);
    return true;
  }

  async function runBinding(binding: Binding, rest: string) {
    const [kind, ...parts] = binding.target.split(':');
    const value = parts.join(':');
    if (kind === 'dest') {
      armedId.value = value;
      query.value = rest;
      inputEl.value?.focus();
    } else if (kind === 'view' || kind === 'scope') {
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

  async function copyLink(text: string, destinationId: string) {
    try {
      await copyText(await resolveUrl(text, destinationId));
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  async function openNote(item: Suggestion) {
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
    if (items.value.length === 0) {
      pendingMove += delta;
      return;
    }
    userMoved = true;
    if (selected.value < 0) {
      selected.value = delta > 0 ? 0 : items.value.length - 1;
      return;
    }
    const count = Math.min(items.value.length, visibleCount.value);
    selected.value = (Math.min(selected.value, count - 1) + delta + count) % count;
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
      if (item.kind === 'answer') {
        await copyText(item.label);
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
        openView(item.destinationId);
        return;
      }
      if (item.kind === 'query') {
        // Below a settings page, the web row is a plain search: picking it also teaches
        // Enter to search this text from now on.
        const first = items.value[0]?.kind === 'query';
        await run(typedText(), first ? armedId.value : item.destinationId, first);
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
      if (item.kind === 'destination') {
        armedId.value = item.destinationId;
        query.value = '';
        return;
      }
      await run(item.query, item.destinationId, false);
      return;
    }

    await run(typedText(), armedId.value, true);
  }

  // Lists never scroll: only the rows that fit are shown (see v-fit), and arrows stay
  // among them.
  const visibleCount = ref(Infinity);
  const actionsShown = ref(Infinity);

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

  /** Shut down, restart and sign out take a second Enter; sleep and lock go at once. */
  async function runPower(preview: PowerPreview) {
    const key = `${preview.action}:${preview.at ?? 'now'}`;
    if (preview.confirm && !preview.at && confirmArm.value !== key) {
      confirmArm.value = key;
      notice.value = `Press Enter again to ${preview.label.toLowerCase()}, or Esc to back out`;
      return;
    }
    confirmArm.value = '';
    try {
      await powerRun(preview.action, preview.at);
      query.value = '';
      notice.value = preview.at ? `${preview.label}. Open Zephyr to cancel it.` : null;
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  async function cancelPower() {
    try {
      await powerCancel();
      query.value = '';
      notice.value = 'Cancelled';
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  async function switchTo(handle: number) {
    try {
      await windowFocus(handle);
      query.value = '';
      scope.value = null;
      selected.value = -1;
      notice.value = null;
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  /** Closing asks the window, so the app can still offer to save; the list catches up. */
  async function closeWindow(handle: number) {
    try {
      await windowClose(handle);
      notice.value = 'Asked the window to close';
      window.setTimeout(scheduleSuggest, 400);
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  async function quitApp(appId: string, name: string) {
    try {
      await appQuit(appId);
      notice.value = `Asked ${name} to quit`;
      window.setTimeout(scheduleSuggest, 400);
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  /**
   * Opens an app; a running one is switched to unless `newWindow` asks for another, and
   * `privateWindow` opens a browser in its private window.
   */
  async function launch(appId: string, newWindow = false, privateWindow = false) {
    try {
      await launchApp(appId, newWindow, privateWindow);
      query.value = '';
      scope.value = null;
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
      scope.value = null;
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
      scope.value = null;
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
      scope.value = null;
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
        scope.value = null;
        selected.value = -1;
        notice.value = null;
        return;
      }
      if (outcome.kind === 'ask') {
        query.value = '';
        scope.value = null;
        void enterAi(outcome.query);
        return;
      }
      if (outcome.kind === 'armed') {
        armedId.value = outcome.destinationId;
        query.value = '';
        scope.value = null;
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
      :title="`Recording · ${isMac ? '⌃⌥R' : 'Ctrl+Alt+R'} stops`"
    />
    <ApprovalCard
      v-if="approvals.length"
      :approval="approvals[0]"
      :queued="approvals.length"
      @answer="answerApproval"
    />
    <DeviceCard
      v-if="!approvals.length && devicesWaiting.length"
      :device="devicesWaiting[0]"
      @answer="answerDevice"
    />
    <ClaudeView
      v-if="visited.claude"
      v-show="claudeMode"
      ref="claudeView"
      :initial-query="claudeQuery"
      :settings="snapshot?.claude ?? null"
      @exit="leave"
    />
    <ShellView
      v-if="visited.shell"
      v-show="shellMode"
      ref="shellView"
      :initial-query="shellQuery"
      :settings="snapshot?.shell ?? null"
      @exit="leave"
    />
    <AiView v-if="visited.ai" v-show="aiMode" ref="aiView" @exit="leave" />
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
      @ask="askFromResources"
      @search="searchFromResources"
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
            aria-controls="results"
            :aria-expanded="items.length > 0"
            :aria-activedescendant="selected >= 0 ? `result-${selected}` : undefined"
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
        <!-- Only when Tab or Ctrl+1–8 sent the text somewhere other than the default; the
             top row already says where it goes. -->
        <button
          v-if="armed && armed.id !== snapshot?.defaultDestinationId"
          class="chip active armed-pill"
          type="button"
          title="Tab cycles · Ctrl+1–8 picks"
          @mousedown.prevent
          @click="cycleArmed(false)"
        >
          {{ armed.name }}
        </button>
        <!-- With Ctrl held the bar can be dragged; otherwise what's playing, if anything. -->
        <span v-if="movable" class="move-hint">
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path d="M12 3v18M3 12h18M12 3 9.5 5.5M12 3l2.5 2.5M12 21l-2.5-2.5M12 21l2.5-2.5" />
            <path d="M3 12l2.5-2.5M3 12l2.5 2.5M21 12l-2.5-2.5M21 12l-2.5 2.5" />
          </svg>
          Drag to move
        </span>
        <NowPlayingStrip
          v-else-if="playingNow"
          compact
          :track="playingNow"
          @toggle="media.press('toggle')"
          @next="media.press('next')"
          @previous="media.press('previous')"
          @focus="media.focus()"
        />
        <button
          type="button"
          class="bar-settings"
          title="Zephyr settings"
          aria-label="Zephyr settings"
          @mousedown.prevent
          @click="openSettings"
        >
          <svg viewBox="0 0 24 24" aria-hidden="true" v-html="GLYPHS.gear" />
        </button>
      </div>

      <p v-if="loadError || notice" class="notice">{{ loadError || notice }}</p>

      <ul
        v-if="actionsOpen"
        class="results actions"
        role="listbox"
        aria-label="Actions"
        v-fit="(count: number) => (actionsShown = count)"
      >
        <li
          v-for="(action, index) in actions"
          :key="action.label"
          role="option"
          :aria-selected="index === actionIndex"
          class="item"
          :class="{ selected: index === actionIndex }"
          @mousedown.prevent
          @click="runAction(action)"
          @mousemove="actionIndex = index"
        >
          <span class="label">{{ action.label }}</span>
          <span v-if="action.hint" class="hint">{{ action.hint }}</span>
        </li>
      </ul>

      <ResultList
        v-else-if="items.length"
        :groups="sections"
        :selected="selected"
        :names="destinationNames"
        :numbered="showNumbers ? RUN_KEY : ''"
        @choose="chooseAt"
        @hover="hover"
        @drag="dragRow"
        @fit="visibleCount = $event"
      />

      <!-- After the list's v-if chain, which it must not join. -->
      <FilePreviewPane
        v-if="previewShown && selectedItem?.path"
        :path="selectedItem.path"
        :name="selectedItem.label"
        :icon="selectedItem.icon"
        :style="previewStyle"
      />

      <footer
        v-if="statsShown && stats"
        ref="statsEl"
        class="stats"
        aria-label="System"
        title="Open System Resources"
        @mousedown.prevent
        @click="openView('resources')"
      >
        <span class="stat" title="CPU">
          <svg viewBox="0 0 24 24" aria-hidden="true" v-html="GLYPHS.cpu" />
          {{ Math.round(stats.cpu) }}%
        </span>
        <span class="stat" title="Memory used">
          <svg viewBox="0 0 24 24" aria-hidden="true" v-html="GLYPHS.memory" />
          {{ bytes(stats.memoryUsed) }} / {{ bytes(stats.memoryTotal, 0) }}
        </span>
        <span v-if="stats.diskTotal" class="stat" title="Free on the system disk">
          <svg viewBox="0 0 24 24" aria-hidden="true" v-html="GLYPHS.drive" />
          {{ bytes(stats.diskFree) }} free
        </span>
        <span class="stat" title="Network, down and up">
          <svg viewBox="0 0 24 24" aria-hidden="true" v-html="GLYPHS.updown" />
          ↓ {{ bytes(stats.netDown) }}/s · ↑ {{ bytes(stats.netUp) }}/s
        </span>
        <span
          v-if="stats.app"
          class="stat stat-app"
          :title="`This dev build: ${stats.app.processes} processes, webview helpers included`"
        >
          <svg viewBox="0 0 24 24" aria-hidden="true" v-html="GLYPHS.logo" />
          {{ Math.round(stats.app.cpu) }}% · {{ bytes(stats.app.memory) }}
        </span>
      </footer>
    </template>
  </div>
</template>
