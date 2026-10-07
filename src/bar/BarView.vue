<script setup lang="ts">
  import { computed, nextTick, onMounted, onUnmounted, reactive, ref, watch } from 'vue';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import {
    claudeAnswer,
    claudeApprovals,
    claudeJobs,
    dispatch,
    errorMessage,
    formatShortcut,
    getSnapshot,
    hideBar,
    launchApp,
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
  } from '../api';
  import ClipboardView from './ClipboardView.vue';
  import TypingView from '../typing/TypingView.vue';
  import AiView from '../ai/AiView.vue';
  import ApprovalCard from '../claude/ApprovalCard.vue';
  import DeviceCard from './DeviceCard.vue';
  import ClaudeView from '../claude/ClaudeView.vue';
  import ShellView from '../shell/ShellView.vue';
  import {
    isSnapshot,
    type ClaudeApproval,
    type ClaudeJob,
    type SyncPending,
    type Destination,
    type Snapshot,
    type Suggestion,
  } from '../types';

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

  // Views that take over the bar. Workspaces (Claude, Ask AI, typing, and Notes in its own
  // window) stick: every open lands back in them until Esc leaves. Esc to the main bar only
  // parks a workspace; a second Esc there clears it. Clipboard is a picker: it sticks until
  // something is picked.
  type View = 'root' | 'claude' | 'clip' | 'typing' | 'ai' | 'shell';
  const WORKSPACES = ['claude', 'ai', 'typing', 'shell'];
  const MODE_NAMES: Record<string, string> = {
    claude: 'Claude jobs',
    ai: 'Ask AI',
    typing: 'typing test',
    shell: 'Shell',
    notes: 'Notes',
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
  const CLIP_WIDTH = 860;
  const clipQuery = ref('');
  const clipView = ref<InstanceType<typeof ClipboardView> | null>(null);
  const TYPING_WIDTH = 760;
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
    if (item?.kind === 'answer') return `copy ${item.label}`;
    if (item?.kind === 'note') return `open ${item.label}`;
    if (item?.kind === 'noteNew') return 'create note';
    if (item?.kind === 'view') return `open ${item.label}`;
    if (item?.kind === 'app' || item?.kind === 'setting' || item?.kind === 'file') {
      return `Open ${item.label}`;
    }
    return armed.value?.name ?? 'search';
  });

  const isMac = navigator.userAgent.includes('Mac');
  const actionKey = isMac ? '⌘K' : 'Ctrl+K';

  // Ctrl+K (⌘K on a Mac) lists what else the selected row, or the typed text, can do.
  const actions = computed<Action[]>(() => {
    const item = selectedItem.value;
    const list: Action[] = [];
    if (item?.kind === 'answer') {
      list.push({ label: 'Copy answer', run: () => copyText(item.label) });
    } else if (item?.kind === 'app' && item.appId) {
      const appId = item.appId;
      list.push({ label: `Open ${item.label}`, run: () => launch(appId) });
      list.push({ label: 'Copy name', run: () => copyText(item.label) });
    } else if (item?.kind === 'file' && item.path) {
      const path = item.path;
      list.push({ label: `Open ${item.label}`, run: () => openPath(path) });
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
          hint: destination.triggers[0] ? `!${destination.triggers[0]}` : undefined,
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

  const shortcutLabel = computed(() =>
    snapshot.value
      ? formatShortcut(snapshot.value.summonShortcut)
      : formatShortcut(isMac ? 'command+space' : 'alt+space')
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
        void loadApprovals();
        void loadJobs();
        void loadDevices();
        if (snapshot.value && !snapshot.value.returnToMode) {
          parked.value = '';
          view.value = 'root';
          resetForSummon();
          void focusInput();
          return;
        }
        // Still inside a view (a workspace, or a picker mid-search): reopen right there.
        if (view.value !== 'root') return;
        if (WORKSPACES.includes(parked.value)) {
          enter(parked.value as View);
          return;
        }
        // The main bar keeps its text only briefly.
        const limit = (snapshot.value?.resumeSeconds ?? 120) * 1000;
        const recent = hiddenAt > 0 && limit > 0 && Date.now() - hiddenAt < limit;
        // A picker search (!f, !app, !set, !note) sticks until something is picked.
        const picker = /^!(f|file|files|app|apps|set|settings|note|notes)\b/i.test(query.value);
        if (!recent && !picker) resetForSummon();
        void focusInput();
      })
    );
    unlistens.push(
      await listen('bar-hidden', () => {
        hiddenAt = Date.now();
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
    window.addEventListener('blur', () => (held.value = ''));
    if (root.value) {
      observer = new ResizeObserver(() => syncHeight());
      observer.observe(root.value);
      syncHeight();
    }
    await focusInput();
    scheduleSuggest();
  });

  onUnmounted(() => {
    window.clearInterval(devicePoll);
    window.removeEventListener('keydown', onKey, true);
    window.removeEventListener('keydown', trackModifiers, true);
    window.removeEventListener('keyup', trackModifiers, true);
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
      void focusInput();
    } else if (view.value === 'ai') {
      void aiView.value?.focus();
    }
  });

  /** Enters a view. Workspaces become the place Zephyr returns to. */
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

  /** Rows on the empty bar that jump straight into a view. */
  function viewRows(): Suggestion[] {
    const rows: Suggestion[] = [];
    const settings = snapshot.value;
    if (parked.value && MODE_NAMES[parked.value]) {
      rows.push(viewRow('back', `Back to ${MODE_NAMES[parked.value]}`, 'Esc again to clear'));
    }
    if (jobs.value.length || settings?.claude.projects.length) {
      const running = jobs.value.filter(
        (job) => job.status === 'running' || job.status === 'waiting' || job.status === 'queued'
      ).length;
      const latest = jobs.value[0];
      const hint = running
        ? `${running} running`
        : latest
          ? `${latest.status === 'failed' ? '✗' : '✓'} ${latest.title}`
          : '!claude';
      rows.push(viewRow('claude', 'Claude jobs', hint));
    }
    if (settings?.clipboard.enabled) {
      rows.push(
        viewRow(
          'clip',
          'Clipboard history',
          settings.clipboard.shortcut ? formatShortcut(settings.clipboard.shortcut) : '!clip'
        )
      );
    }
    rows.push(
      viewRow(
        'notes',
        'Notes',
        settings?.notesShortcut ? formatShortcut(settings.notesShortcut) : '!note'
      )
    );
    return rows;
  }

  function viewRow(id: string, label: string, hint: string): Suggestion {
    return { label, hint, query: '', destinationId: id, kind: 'view' };
  }

  function openView(id: string) {
    const typed = query.value;
    query.value = '';
    if (id === 'back') id = parked.value;
    if (id === 'shell') {
      enterWith('shell', id === parked.value ? '' : typed.trim().replace(/^\$\s+/, ''));
    } else if (id === 'claude' || id === 'ai' || id === 'typing') {
      enter(id);
    } else if (id === 'clip') {
      clipQuery.value = '';
      enter('clip');
    } else if (id === 'notes') {
      parked.value = 'notes';
      void openNotes();
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

  /** Clears the main bar's text and selection; views and the parked workspace are untouched. */
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
    const width =
      view.value === 'clip' ? CLIP_WIDTH : view.value === 'root' ? undefined : TYPING_WIDTH;
    void setBarHeight(height, width).catch(() => undefined);
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
      const views = response.mode === 'recent' && !query.value.trim() ? viewRows() : [];
      const shellRow = looksLikeCommand(query.value)
        ? [viewRow('shell', 'Run in shell', '> or !sh')]
        : [];
      items.value = [...views, ...response.items, ...shellRow];
      mode.value = response.mode;
      notice.value = response.notice;
      if (!userMoved) {
        selected.value = response.preselect === null ? -1 : response.preselect + views.length;
      }
      await nextTick();
      syncHeight();
    } catch (error) {
      if (current !== generation) return;
      notice.value = errorMessage(error);
    }
  }

  function onKey(event: KeyboardEvent) {
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
    if (view.value === 'claude') {
      claudeView.value?.onKey(event);
      return;
    }
    if (view.value === 'clip') {
      clipView.value?.onKey(event);
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
        const count = actions.value.length;
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

    if (event.key === 'Enter') {
      event.preventDefault();
      if (event.repeat || event.shiftKey) return;
      if (modifier || event.altKey) {
        void route(event.altKey ? 'alt' : 'cmd');
        return;
      }
      void accept();
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
        ? { kind: 'ai', id: chosen.id, label: 'Ask AI' }
        : { kind: 'web', id: chosen.id, label: chosen.name };
    }
    if (which === 'alt') return claudeRoute('');
    // The other primary action: AI when a web destination is armed, and the reverse.
    if (armed.value?.kind === 'ai') {
      const id = webDefault();
      return {
        kind: 'web',
        id,
        label: destinations.find((item) => item.id === id)?.name ?? 'search',
      };
    }
    return { kind: 'ai', id: 'ai', label: 'Ask AI' };
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
  function trackModifiers(event: KeyboardEvent) {
    held.value = event.altKey ? 'alt' : event.metaKey || event.ctrlKey ? 'cmd' : '';
  }
  const cmdLabel = computed(() => routeFor('cmd').label);
  const altLabel = computed(() => routeFor('alt').label);

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

  async function copyLink(text: string, destinationId: string) {
    try {
      await copyText(await resolveUrl(text, destinationId));
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  async function openNote(item: Suggestion) {
    parked.value = 'notes';
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
      if (item.kind === 'answer') {
        await copyText(item.label);
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
    if (item.kind === 'answer') {
      void copyText(item.label);
      return;
    }
    if (item.kind === 'note' || item.kind === 'noteNew') {
      void openNote(item);
      return;
    }
    if (item.kind === 'view') {
      openView(item.destinationId);
      return;
    }
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
      if (outcome.kind === 'ask') {
        query.value = '';
        void enterAi(outcome.query);
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
  <div
    ref="root"
    class="bar"
    :class="{ wide: clipMode, 'typing-wide': view !== 'root' && !clipMode }"
    role="dialog"
    aria-label="Zephyr"
  >
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
    <ClipboardView
      v-if="clipMode"
      ref="clipView"
      :initial-query="clipQuery"
      @exit="leave"
      @picked="leave"
    />
    <template v-if="view === 'root'">
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

      <ul v-if="actionsOpen" class="results actions" role="listbox" aria-label="Actions">
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

      <ul v-else-if="items.length" id="results" class="results" role="listbox">
        <li
          v-for="(item, index) in items"
          :id="`result-${index}`"
          :key="`${item.kind}-${item.destinationId}-${item.path ?? item.label}`"
          role="option"
          :aria-selected="index === selected"
          class="item"
          :class="{ selected: index === selected, answer: item.kind === 'answer' }"
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
          <span :class="{ dim: held }">↵ {{ enterLabel }}</span>
          <template v-if="query.trim()">
            ·
            <span :class="{ lit: held === 'cmd' }"
              >{{ isMac ? '⌘' : 'Ctrl+' }}↵ {{ cmdLabel }}</span
            >
            ·
            <span :class="{ lit: held === 'alt' }">{{ isMac ? '⌥' : 'Alt+' }}↵ {{ altLabel }}</span>
          </template>
          <template v-if="selectedItem?.kind === 'file'">
            · Ctrl+Enter show in {{ isMac ? 'Finder' : 'Explorer' }} · Ctrl+Shift+C copy path
          </template>
          <template v-if="actions.length && !actionsOpen"> · {{ actionKey }} actions</template>
          <template v-if="actionsOpen"> · Esc back</template>
        </span>
        <span>{{ shortcutLabel }}</span>
      </footer>
    </template>
  </div>
</template>
