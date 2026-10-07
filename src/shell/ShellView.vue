<script setup lang="ts">
  import { computed, nextTick, onMounted, ref, watch } from 'vue';
  import {
    errorMessage,
    openSetting,
    shellInfo,
    shellOpenTerminal,
    shellRun,
    shellStop,
  } from '../api';
  import type { ShellSettings } from '../types';

  const props = defineProps<{ initialQuery: string; settings: ShellSettings | null }>();
  const emit = defineEmits<{ exit: [] }>();

  interface Run {
    id: number | null;
    command: string;
    shell: string;
    output: string;
    status: 'running' | 'done' | 'failed' | 'stopped' | 'error';
    code: number | null;
    millis: number;
  }

  interface Action {
    label: string;
    keys?: string;
    run: () => unknown;
  }

  /** Output kept per run; older text is dropped from the top. */
  const OUTPUT_LIMIT = 200_000;

  const isMac = navigator.userAgent.includes('Mac');
  const mod = isMac ? '⌘' : 'Ctrl+';

  const inputEl = ref<HTMLInputElement | null>(null);
  const outputEl = ref<HTMLElement | null>(null);
  const text = ref(props.initialQuery);
  const run = ref<Run | null>(null);
  const shellName = ref<string | null>(null);
  const terminalName = ref('terminal');
  const selected = ref(-1);
  const notice = ref<string | null>(null);
  /** The folder the last command ended in, so a `cd` carries over; null is home. */
  const cwd = ref<string | null>(null);
  const actionsOpen = ref(false);
  const actionIndex = ref(0);
  let generation = 0;

  const running = computed(() => run.value?.status === 'running');

  /** Past commands matching what is typed, newest first; hidden while output is showing. */
  const recent = computed(() => {
    const typed = text.value.trim().toLowerCase();
    const history = props.settings?.history ?? [];
    if (run.value && (!typed || typed === run.value.command.toLowerCase())) return [];
    return history
      .filter((command) => !typed || command.toLowerCase().includes(typed))
      .filter((command) => command !== text.value.trim())
      .slice(0, 6);
  });

  const target = computed(() =>
    selected.value >= 0 ? recent.value[selected.value] : text.value.trim()
  );

  const output = computed(() => clean(run.value?.output ?? ''));

  /** The last two parts of the working folder, or ~ for home. */
  const folderLabel = computed(() => {
    if (!cwd.value) return '~';
    const parts = cwd.value.split(/[\\/]/).filter(Boolean);
    return parts.length > 2 ? `…/${parts.slice(-2).join('/')}` : cwd.value;
  });

  const status = computed(() => {
    const current = run.value;
    if (!current) return '';
    const time = seconds(current.millis);
    switch (current.status) {
      case 'running':
        return 'Running…';
      case 'done':
        return `Exit 0 · ${time}`;
      case 'failed':
        return `Exit ${current.code ?? '?'} · ${time}`;
      case 'stopped':
        return `Stopped · ${time}`;
      default:
        return 'Couldn’t run it';
    }
  });

  const actions = computed<Action[]>(() => {
    const list: Action[] = [];
    const current = run.value;
    if (current?.status === 'running') {
      list.push({ label: 'Stop', keys: `${mod}C`, run: stop });
    }
    if (current?.output) list.push({ label: 'Copy output', run: copyOutput });
    if (current && current.status !== 'running') {
      list.push({ label: 'Run again', run: () => start(current.command) });
    }
    list.push({
      label: `Open in ${terminalName.value}`,
      keys: `${mod}↵`,
      run: () => openTerminal(target.value),
    });
    if (current && current.status !== 'running') {
      list.push({ label: 'Clear output', run: () => (run.value = null) });
    }
    list.push({ label: 'Choose the shell', run: () => openSetting('zephyr.shell') });
    return list;
  });

  onMounted(async () => {
    try {
      const info = await shellInfo();
      shellName.value = info.using;
      terminalName.value = info.terminal;
    } catch {
      shellName.value = null;
    }
    await nextTick();
    inputEl.value?.focus();
  });

  watch(text, () => {
    selected.value = -1;
    notice.value = null;
    actionsOpen.value = false;
  });

  function seconds(millis: number): string {
    if (millis < 1000) return `${millis} ms`;
    if (millis < 60_000) return `${(millis / 1000).toFixed(1)} s`;
    return `${Math.floor(millis / 60_000)}m ${Math.round((millis % 60_000) / 1000)}s`;
  }

  // The run script's last line: the folder it ended in (shell.rs).
  // eslint-disable-next-line no-control-regex
  const CWD_LINE = /\n?\x1e\x1eZCWD:([^\n]*)\n?/g;

  function endedIn(raw: string): string | null {
    const found = [...raw.matchAll(CWD_LINE)].pop();
    return found ? found[1].trim() || null : null;
  }

  /** Drops color codes and keeps only the last state of lines redrawn with a carriage return. */
  function clean(raw: string): string {
    const plain = raw
      .replace(CWD_LINE, '')
      // eslint-disable-next-line no-control-regex
      .replace(/\x1b\[[0-?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/g, '');
    return plain
      .replace(/\r\n/g, '\n')
      .split('\n')
      .map((line) => (line.includes('\r') ? line.slice(line.lastIndexOf('\r') + 1) : line))
      .join('\n');
  }

  async function start(command: string) {
    const value = command.trim();
    if (!value) return;
    if (running.value) {
      notice.value = `Still running · ${mod}C stops it`;
      return;
    }
    const current = ++generation;
    text.value = value;
    await nextTick();
    selected.value = -1;
    run.value = {
      id: null,
      command: value,
      shell: shellName.value ?? '',
      output: '',
      status: 'running',
      code: null,
      millis: 0,
    };
    const mine = () => (current === generation ? run.value : null);
    try {
      await shellRun(value, cwd.value, (event) => {
        const target = mine();
        if (!target) return;
        if (event.kind === 'started') {
          target.id = event.id;
          target.shell = event.shell;
        } else if (event.kind === 'output') {
          target.output = (target.output + event.text).slice(-OUTPUT_LIMIT);
          void follow();
        } else if (event.kind === 'exit') {
          target.code = event.code;
          target.millis = event.millis;
          cwd.value = endedIn(target.output) ?? cwd.value;
          target.status = event.stopped ? 'stopped' : event.code === 0 ? 'done' : 'failed';
        } else {
          target.status = 'error';
          target.output += `\n${event.message}`;
        }
      });
    } catch (error) {
      const target = mine();
      if (target) {
        target.status = 'error';
        target.output = errorMessage(error);
      }
    }
  }

  // Keep the newest output in view unless the user scrolled up to read.
  async function follow() {
    const el = outputEl.value;
    const pinned = !el || el.scrollHeight - el.scrollTop - el.clientHeight < 24;
    await nextTick();
    if (pinned && outputEl.value) outputEl.value.scrollTop = outputEl.value.scrollHeight;
  }

  async function stop() {
    const id = run.value?.id;
    if (id != null && running.value) await shellStop(id).catch(() => undefined);
  }

  async function copyOutput() {
    try {
      await navigator.clipboard.writeText(output.value.trim());
      notice.value = 'Copied the output';
    } catch {
      notice.value = "Couldn't copy the output";
    }
  }

  async function openTerminal(command: string) {
    try {
      await shellOpenTerminal(command, cwd.value);
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  function runAction(action: Action | undefined) {
    if (!action) return;
    actionsOpen.value = false;
    void action.run();
  }

  function move(delta: number) {
    const count = recent.value.length;
    if (!count) return;
    if (selected.value < 0) selected.value = delta > 0 ? 0 : count - 1;
    else {
      const next = selected.value + delta;
      selected.value = next < 0 || next >= count ? -1 : next;
    }
  }

  /** Called by the bar for every key while the shell view is showing. */
  function onKey(event: KeyboardEvent) {
    const modifier = event.metaKey || event.ctrlKey;
    if (modifier && event.code === 'KeyK') {
      event.preventDefault();
      actionsOpen.value = !actionsOpen.value && actions.value.length > 0;
      actionIndex.value = 0;
      return;
    }
    if (actionsOpen.value) {
      if (event.key === 'Escape') {
        event.preventDefault();
        actionsOpen.value = false;
      } else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
        event.preventDefault();
        const count = actions.value.length;
        actionIndex.value =
          (actionIndex.value + (event.key === 'ArrowDown' ? 1 : -1) + count) % count;
      } else if (event.key === 'Enter') {
        event.preventDefault();
        if (!event.repeat) runAction(actions.value[actionIndex.value]);
      }
      return;
    }
    // Ctrl+C stops a running command unless text in the box is selected to copy.
    if (modifier && event.code === 'KeyC' && running.value) {
      const input = inputEl.value;
      const hasSelection = input && input.selectionStart !== input.selectionEnd;
      if (!hasSelection) {
        event.preventDefault();
        void stop();
        return;
      }
    }
    if (event.key === 'Escape') {
      event.preventDefault();
      emit('exit');
      return;
    }
    if (event.key === 'Backspace' && !event.repeat && text.value === '' && !run.value) {
      event.preventDefault();
      emit('exit');
      return;
    }
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      move(event.key === 'ArrowDown' ? 1 : -1);
      return;
    }
    if (event.key === 'Tab' && selected.value >= 0) {
      event.preventDefault();
      text.value = recent.value[selected.value];
      return;
    }
    if (event.key === 'Enter') {
      event.preventDefault();
      if (event.repeat) return;
      // Commands run here; Ctrl+Enter is the way out to a real terminal for interactive ones.
      if (modifier) void openTerminal(target.value);
      else void start(target.value);
    }
  }

  defineExpose({ onKey, text });
</script>

<template>
  <div class="shell">
    <div class="input-row">
      <span class="shell-mark" aria-hidden="true">❯</span>
      <input
        ref="inputEl"
        v-model="text"
        class="query shell-input"
        type="text"
        placeholder="Type or paste a command"
        spellcheck="false"
        autocomplete="off"
        autocapitalize="off"
      />
      <button
        type="button"
        class="chip active shell-cwd"
        :title="`${cwd ?? '~'} · type cd to change it`"
        @mousedown.prevent
        @click="inputEl?.focus()"
      >
        {{ folderLabel }}
      </button>
    </div>

    <p v-if="notice" class="notice">{{ notice }}</p>

    <ul v-if="actionsOpen" class="results actions" role="listbox" aria-label="Shell actions">
      <li
        v-for="(action, index) in actions"
        :key="action.label"
        class="item"
        :class="{ selected: index === actionIndex }"
        @mousedown.prevent
        @click="runAction(action)"
        @mousemove="actionIndex = index"
      >
        <span class="label">{{ action.label }}</span>
        <span v-if="action.keys" class="hint">{{ action.keys }}</span>
      </li>
    </ul>

    <template v-else>
      <section v-if="run" class="shell-run" :class="run.status" aria-live="polite">
        <!-- prettier-ignore -->
        <pre ref="outputEl" class="shell-output">{{ output || (run.status === 'running' ? '' : 'No output') }}<span v-if="run.status === 'running'" class="caret" /></pre>
        <p class="shell-status">{{ status }}</p>
      </section>

      <ul v-if="recent.length" class="results" role="listbox" aria-label="Recent commands">
        <li
          v-for="(command, index) in recent"
          :key="command"
          class="item"
          :class="{ selected: index === selected }"
          @mousedown.prevent
          @click="text = command"
          @mousemove="selected = index"
        >
          <span class="label shell-command">{{ command }}</span>
          <span v-if="index === selected" class="hint">Tab edits</span>
        </li>
      </ul>
      <p v-else-if="!run && !text.trim()" class="empty">Commands you run will show up here.</p>
    </template>

    <footer class="footer">
      <span>
        ↵ {{ running ? 'busy' : 'run' }} · {{ mod }}↵ open in {{ terminalName }} · {{ mod }}K
        actions
        <template v-if="running"> · {{ mod }}C stop</template>
      </span>
      <button
        type="button"
        class="text-button"
        title="Choose the shell"
        @mousedown.prevent
        @click="openSetting('zephyr.shell')"
      >
        {{ run?.shell || shellName || 'Shell' }}
      </button>
    </footer>
  </div>
</template>
