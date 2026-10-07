<script setup lang="ts">
  import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import {
    claudeAnswer,
    claudeApprovals,
    claudeJobs,
    aiAsk,
    aiCancel,
    dispatch,
    errorMessage,
    formatShortcut,
    getSnapshot,
    hideBar,
    launchApp,
    openFile,
    openSetting,
    openNotes,
    openSettings,
    resolveUrl,
    revealFile,
    setBarHeight,
    suggest,
  } from '../api';
  import ClipboardView from './ClipboardView.vue';
  import TypingView from '../typing/TypingView.vue';
  import ShellView from '../shell/ShellView.vue';
  import ApprovalCard from '../claude/ApprovalCard.vue';
  import ClaudeView from '../claude/ClaudeView.vue';
  import {
    isSnapshot,
    type ClaudeApproval,
    type ClaudeJob,
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

  // Clipboard history takes over the bar: from its hotkey, or by typing !clip.
  const CLIP_WIDTH = 860;
  const clipMode = ref(false);
  const clipQuery = ref('');
  const clipView = ref<InstanceType<typeof ClipboardView> | null>(null);
  // The typing test also takes over the bar, from !type.
  const TYPING_WIDTH = 760;
  const typingMode = ref(false);
  const typingView = ref<InstanceType<typeof TypingView> | null>(null);
  // Background Claude jobs, from !claude; pending permission requests show above everything.
  const claudeMode = ref(false);
  const claudeQuery = ref('');
  const claudeView = ref<InstanceType<typeof ClaudeView> | null>(null);
  // Shell commands, from > or !sh; a command never runs on unscoped Enter.
  const shellMode = ref(false);
  const shellQuery = ref('');
  const shellView = ref<InstanceType<typeof ShellView> | null>(null);
  const SHELL_PREFIX = /^\s*(?:>|!(?:sh|shell)(?:\s|$))\s*/i;
  const approvals = ref<ClaudeApproval[]>([]);
  const jobs = ref<ClaudeJob[]>([]);

  // Reopening soon after closing returns to the view and text it closed on.
  let hiddenAt = 0;

  interface Answer {
    question: string;
    input: string;
    text: string;
    status: 'waiting' | 'streaming' | 'done' | 'error';
    error: string | null;
    source: string | null;
  }

  interface Action {
    label: string;
    hint?: string;
    run: () => unknown;
  }

  const actionsOpen = ref(false);
  const actionIndex = ref(0);

  const answer = ref<Answer | null>(null);
  const answerEl = ref<HTMLElement | null>(null);
  let answerGeneration = 0;

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

  // The answer stays up until the text changes; Enter then copies it instead of asking again.
  const answerReady = computed(
    () => answer.value?.status === 'done' && query.value === answer.value.input
  );

  const enterLabel = computed(() => {
    if (answerReady.value) return 'copy answer';
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
    if (answerReady.value) {
      list.push({ label: 'Copy answer', run: () => copyAnswer() });
    }
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
    } catch (error) {
      loadError.value = errorMessage(error);
    }

    unlistens.push(
      await listen('bar-shown', () => {
        const limit = (snapshot.value?.resumeSeconds ?? 120) * 1000;
        const resume = hiddenAt > 0 && limit > 0 && Date.now() - hiddenAt < limit;
        if (resume) {
          void loadApprovals();
          void loadJobs();
        } else {
          resetForSummon();
        }
        if (!anyView()) void focusInput();
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
    unlistens.push(
      await listen('clipboard-shown', () => {
        resetForSummon();
        clipQuery.value = '';
        clipMode.value = true;
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

  watch(selected, () => {
    actionsOpen.value = false;
  });

  watch([clipMode, typingMode, claudeMode, shellMode], async () => {
    await nextTick();
    syncHeight();
    if (!anyView()) void focusInput();
  });

  function anyView(): boolean {
    return clipMode.value || typingMode.value || claudeMode.value || shellMode.value;
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
      rows.push(view('claude', 'Claude jobs', hint));
    }
    if (settings?.clipboard.enabled) {
      rows.push(
        view(
          'clip',
          'Clipboard history',
          settings.clipboard.shortcut ? formatShortcut(settings.clipboard.shortcut) : '!clip'
        )
      );
    }
    rows.push(
      view(
        'notes',
        'Notes',
        settings?.notesShortcut ? formatShortcut(settings.notesShortcut) : '!note'
      )
    );
    return rows;
  }

  function view(id: string, label: string, hint: string): Suggestion {
    return { label, hint, query: '', destinationId: id, kind: 'view' };
  }

  function openView(id: string) {
    const typed = query.value;
    query.value = '';
    if (id === 'shell') {
      shellQuery.value = typed.trim().replace(/^\$\s+/, '');
      shellMode.value = true;
    } else if (id === 'claude') {
      claudeQuery.value = '';
      claudeMode.value = true;
    } else if (id === 'clip') {
      clipQuery.value = '';
      clipMode.value = true;
    } else if (id === 'notes') {
      void openNotes();
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
    const typed = claudeMode.value ? (claudeView.value?.text ?? '') : query.value;
    try {
      await claudeAnswer(approval.id, decision, decision === 'deny' ? typed : undefined);
      if (decision === 'deny' && typed) {
        if (claudeMode.value && claudeView.value) claudeView.value.text = '';
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
      typingMode.value = true;
      return;
    }
    // > opens the shell at once; !sh and !shell wait for a space so !shop still works.
    const shell = SHELL_PREFIX.exec(query.value);
    if (shell && (query.value.trimStart().startsWith('>') || /\s$/.test(shell[0]))) {
      shellQuery.value = query.value.slice(shell[0].length);
      query.value = '';
      shellMode.value = true;
      return;
    }
    const claude = /^!claude(?:\s+(.*))?$/i.exec(query.value);
    if (claude) {
      claudeQuery.value = claude[1] ?? '';
      query.value = '';
      claudeMode.value = true;
      return;
    }
    const clip = /^!clip(?:\s+(.*))?$/i.exec(query.value);
    if (clip) {
      clipQuery.value = clip[1] ?? '';
      query.value = '';
      clipMode.value = true;
      return;
    }
    actionsOpen.value = false;
    if (answer.value && query.value !== answer.value.input) clearAnswer();
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
    clipMode.value = false;
    typingMode.value = false;
    claudeMode.value = false;
    shellMode.value = false;
    void loadApprovals();
    clearAnswer();
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
    const width = clipMode.value
      ? CLIP_WIDTH
      : typingMode.value || claudeMode.value || shellMode.value
        ? TYPING_WIDTH
        : undefined;
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
        ? [view('shell', 'Run in shell', '> or !sh')]
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
    if (claudeMode.value) {
      claudeView.value?.onKey(event);
      return;
    }
    if (shellMode.value) {
      shellView.value?.onKey(event);
      return;
    }
    if (clipMode.value) {
      clipView.value?.onKey(event);
      return;
    }
    if (typingMode.value) {
      typingView.value?.onKey(event);
      return;
    }
    // !type then Enter opens the typing test too.
    if (event.key === 'Enter' && /^!(?:sh|shell)$/i.test(query.value.trim())) {
      event.preventDefault();
      query.value = '';
      shellQuery.value = '';
      shellMode.value = true;
      return;
    }
    if (event.key === 'Enter' && /^!type$/i.test(query.value.trim())) {
      event.preventDefault();
      query.value = '';
      typingMode.value = true;
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
      if (answer.value && query.value === answer.value.input) {
        if (answerReady.value) void copyAnswer();
        return;
      }
      void accept();
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

  async function copyLink(text: string, destinationId: string) {
    try {
      await copyText(await resolveUrl(text, destinationId));
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  function clearAnswer() {
    if (!answer.value) return;
    answerGeneration++;
    answer.value = null;
    void aiCancel().catch(() => undefined);
  }

  async function startAnswer(question: string) {
    const generation = ++answerGeneration;
    if (!query.value.trim()) query.value = question;
    answer.value = {
      question,
      input: query.value,
      text: '',
      status: 'waiting',
      error: null,
      source: null,
    };
    selected.value = -1;
    notice.value = null;
    const current = () => (generation === answerGeneration ? answer.value : null);
    try {
      await aiAsk(question, (event) => {
        const target = current();
        if (!target) return;
        if (event.kind === 'started') {
          target.source = `${event.model} · ${event.provider}`;
        } else if (event.kind === 'delta') {
          target.text += event.text;
          target.status = 'streaming';
          void followAnswer();
        } else if (event.kind === 'done') {
          target.status = 'done';
        } else {
          target.status = 'error';
          target.error = event.message;
        }
      });
    } catch (error) {
      const target = current();
      if (target) {
        target.status = 'error';
        target.error = errorMessage(error);
      }
    }
  }

  // Keep the newest text in view unless the user scrolled up to read.
  async function followAnswer() {
    const el = answerEl.value;
    const pinned = !el || el.scrollHeight - el.scrollTop - el.clientHeight < 24;
    await nextTick();
    if (pinned && answerEl.value) answerEl.value.scrollTop = answerEl.value.scrollHeight;
  }

  async function copyAnswer() {
    const text = answer.value?.text.trim();
    if (text) await copyText(text);
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
        void startAnswer(outcome.query);
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
    :class="{ wide: clipMode, 'typing-wide': typingMode }"
    role="dialog"
    aria-label="Zephyr"
  >
    <ApprovalCard
      v-if="approvals.length"
      :approval="approvals[0]"
      :queued="approvals.length"
      @answer="answerApproval"
    />
    <ShellView
      v-if="shellMode"
      ref="shellView"
      :initial-query="shellQuery"
      :settings="snapshot?.shell ?? null"
      @exit="shellMode = false"
    />
    <ClaudeView
      v-else-if="claudeMode"
      ref="claudeView"
      :initial-query="claudeQuery"
      :settings="snapshot?.claude ?? null"
      @exit="claudeMode = false"
    />
    <TypingView
      v-else-if="typingMode"
      ref="typingView"
      :bests="snapshot?.typingBests ?? []"
      @exit="typingMode = false"
    />
    <ClipboardView
      v-else-if="clipMode"
      ref="clipView"
      :initial-query="clipQuery"
      @exit="clipMode = false"
    />
    <template v-else>
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

      <section v-else-if="answer" class="answer" aria-live="polite">
        <div ref="answerEl" class="answer-body">
          <p v-if="answer.status === 'waiting'" class="answer-wait">Thinking…</p>
          <!-- prettier-ignore -->
          <div v-else-if="answer.text" class="answer-text">{{ answer.text }}<span v-if="answer.status === 'streaming'" class="caret" /></div>
          <p v-if="answer.error" class="answer-error">{{ answer.error }}</p>
        </div>
        <p v-if="answer.source" class="answer-source">{{ answer.source }}</p>
      </section>

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
          Enter {{ enterLabel }}
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
