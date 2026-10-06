<script setup lang="ts">
  import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import {
    claudeCancel,
    claudeFollowUp,
    claudeJobs,
    claudeOpenTerminal,
    claudeRemove,
    claudeSubmit,
    errorMessage,
    hideBar,
    openSetting,
  } from '../api';
  import type { ClaudeJob, ClaudeSettings, ClaudeStatus } from '../types';

  const props = defineProps<{ initialQuery: string; settings: ClaudeSettings | null }>();
  const emit = defineEmits<{ exit: [] }>();

  interface Action {
    label: string;
    keys?: string;
    run: () => unknown;
  }

  const isMac = navigator.userAgent.includes('Mac');
  const mod = isMac ? '⌘' : 'Ctrl+';

  const inputEl = ref<HTMLInputElement | null>(null);
  const text = ref(props.initialQuery);
  const jobs = ref<ClaudeJob[]>([]);
  const selected = ref(-1);
  const pickedProject = ref<string | null>(null);
  const notice = ref<string | null>(null);
  const actionsOpen = ref(false);
  const actionIndex = ref(0);
  const detailOpen = ref(false);
  const now = ref(Date.now());

  let unlisten: UnlistenFn | null = null;
  let clock = 0;

  const projects = computed(() => props.settings?.projects ?? []);
  const job = computed(() => (selected.value >= 0 ? jobs.value[selected.value] : undefined));

  /** The project a new task goes to: picked with Ctrl+number, an alias, or the last used. */
  const target = computed(() => {
    const list = projects.value;
    if (pickedProject.value) return list.find((project) => project.id === pickedProject.value);
    const first = text.value.trim().split(/\s+/)[0]?.toLowerCase() ?? '';
    const aliased = list.find((project) => project.alias && project.alias === first);
    return aliased ?? list.find((project) => project.id === props.settings?.lastProject) ?? list[0];
  });

  const followTarget = computed(() => {
    if (text.value.trimStart().startsWith('>')) return jobs.value[0];
    return job.value;
  });

  const enterLabel = computed(() => {
    if (actionsOpen.value) return 'run action';
    if (followTarget.value && text.value.trim())
      return `follow up on “${followTarget.value.title}”`;
    if (job.value && !text.value.trim()) return detailOpen.value ? 'hide details' : 'show details';
    if (text.value.trim() && target.value) return `run in ${projectName(target.value.id)}`;
    return 'type a task';
  });

  const actions = computed<Action[]>(() => {
    const item = job.value;
    if (!item) return [];
    const running = item.status === 'running' || item.status === 'waiting';
    const list: Action[] = [
      { label: 'Follow up', keys: 'type, then ↵', run: () => inputEl.value?.focus() },
      {
        label: detailOpen.value ? 'Hide details' : 'Show details',
        keys: '↵',
        run: () => (detailOpen.value = !detailOpen.value),
      },
      { label: 'Open in terminal', run: () => openTerminal(item.id) },
    ];
    if (running || item.status === 'queued') {
      list.push({ label: 'Cancel', run: () => cancel(item.id) });
    } else {
      list.push({ label: 'Remove from list', run: () => remove(item.id) });
    }
    return list;
  });

  onMounted(async () => {
    await refresh();
    unlisten = await listen('claude-changed', () => void refresh());
    clock = window.setInterval(() => (now.value = Date.now()), 1000);
    await nextTick();
    inputEl.value?.focus();
  });

  onBeforeUnmount(() => {
    unlisten?.();
    window.clearInterval(clock);
  });

  watch(text, () => {
    notice.value = null;
    actionsOpen.value = false;
  });

  watch(selected, () => {
    detailOpen.value = false;
    actionsOpen.value = false;
  });

  async function refresh() {
    try {
      const keep = job.value?.id;
      jobs.value = await claudeJobs();
      if (keep) selected.value = jobs.value.findIndex((item) => item.id === keep);
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  function projectName(id: string): string {
    const project = projects.value.find((item) => item.id === id);
    if (!project) return 'a project';
    return project.alias || project.folder.split(/[\\/]/).filter(Boolean).pop() || project.folder;
  }

  async function submit() {
    const value = text.value.trim();
    if (!value) return;
    try {
      const follow = followTarget.value;
      if (follow) {
        await claudeFollowUp(follow.id, value.replace(/^>\s*/, ''));
      } else {
        if (!projects.value.length) {
          notice.value = 'Add a project folder in Settings > Claude first.';
          return;
        }
        await claudeSubmit(value, pickedProject.value ?? undefined);
      }
      text.value = '';
      selected.value = -1;
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  async function cancel(id: string) {
    try {
      await claudeCancel(id);
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  async function remove(id: string) {
    try {
      await claudeRemove(id);
      selected.value = -1;
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  async function openTerminal(id: string) {
    try {
      await claudeOpenTerminal(id);
      await hideBar();
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
    const count = jobs.value.length;
    if (!count) return;
    if (selected.value < 0) selected.value = delta > 0 ? 0 : count - 1;
    else {
      const next = selected.value + delta;
      selected.value = next < 0 || next >= count ? -1 : next;
    }
  }

  /** Called by the bar for every key while the Claude view is showing. */
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
    if (event.key === 'Escape') {
      event.preventDefault();
      emit('exit');
      return;
    }
    if (event.key === 'Backspace' && text.value === '' && selected.value < 0) {
      event.preventDefault();
      if (pickedProject.value) pickedProject.value = null;
      else emit('exit');
      return;
    }
    if (event.ctrlKey && !event.metaKey && /^Digit[1-8]$/.test(event.code)) {
      const project = projects.value[Number(event.code.slice(5)) - 1];
      if (project) {
        event.preventDefault();
        pickedProject.value = project.id;
      }
      return;
    }
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      move(event.key === 'ArrowDown' ? 1 : -1);
      return;
    }
    if (event.key === 'Enter') {
      event.preventDefault();
      if (event.repeat) return;
      if (!text.value.trim() && job.value) {
        detailOpen.value = !detailOpen.value;
        return;
      }
      void submit();
    }
  }

  function glyph(status: ClaudeStatus): string {
    return {
      running: '●',
      waiting: '?',
      done: '✓',
      failed: '✗',
      queued: '◌',
      cancelled: '–',
      interrupted: '!',
    }[status];
  }

  function elapsed(item: ClaudeJob): string {
    const first = item.turns.find((turn) => turn.started)?.started;
    if (!first) return '';
    const active = item.status === 'running' || item.status === 'waiting';
    const end = active ? now.value / 1000 : item.updated;
    const seconds = Math.max(0, Math.round(end - first));
    if (seconds < 60) return `${seconds}s`;
    if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
    return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`;
  }

  defineExpose({ onKey, text });
</script>

<template>
  <div class="claude">
    <div class="input-row">
      <span class="claude-mark" aria-hidden="true">✳</span>
      <button
        v-if="target"
        type="button"
        class="chip active claude-project"
        :title="'Ctrl+1–8 picks a project'"
        @mousedown.prevent
        @click="pickedProject = null"
      >
        {{ followTarget && text.trim() ? followTarget.project : projectName(target.id) }}
      </button>
      <input
        ref="inputEl"
        v-model="text"
        class="query"
        type="text"
        :placeholder="
          projects.length
            ? 'Describe a bug or a feature — Claude works on it in the background'
            : 'Add a project in Settings > Claude to start'
        "
        spellcheck="false"
        autocomplete="off"
      />
    </div>

    <p v-if="notice" class="notice">{{ notice }}</p>
    <div v-if="!projects.length" class="empty">
      Pick a folder for Claude to work in.
      <button type="button" class="text-button" @click="openSetting('zephyr.claude')">
        Open Settings
      </button>
    </div>

    <ul v-if="actionsOpen" class="results actions" role="listbox" aria-label="Job actions">
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

    <ul v-else-if="jobs.length" class="results claude-jobs" role="listbox" aria-label="Claude jobs">
      <li
        v-for="(item, index) in jobs.slice(0, 8)"
        :key="item.id"
        class="item claude-job"
        :class="{ selected: index === selected, [item.status]: true }"
        @mousedown.prevent
        @click="selected = selected === index ? -1 : index"
      >
        <span class="claude-glyph" aria-hidden="true">{{ glyph(item.status) }}</span>
        <span class="claude-job-text">
          <span class="label">{{ item.title }}</span>
          <span class="claude-activity">
            {{ item.project }} ·
            {{ item.status === 'done' && item.summary ? item.summary : item.activity }}
          </span>
        </span>
        <span class="hint">{{ elapsed(item) }}</span>
      </li>
    </ul>

    <section v-if="detailOpen && job && !actionsOpen" class="claude-detail">
      <template v-for="(turn, index) in job.turns" :key="index">
        <p class="claude-prompt">› {{ turn.prompt }}</p>
        <pre v-if="turn.result" class="claude-result">{{ turn.result }}</pre>
        <p v-if="turn.denials.length" class="claude-denials">
          Denied: {{ turn.denials.join(', ') }}
        </p>
      </template>
    </section>

    <footer class="footer">
      <span>↵ {{ enterLabel }} · ↑↓ jobs · {{ mod }}K actions · &gt; follows up the latest</span>
      <span>{{ jobs.filter((item) => item.status === 'running').length }} running</span>
    </footer>
  </div>
</template>
