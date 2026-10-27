<script setup lang="ts">
  import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import {
    bindingFailures,
    errorMessage,
    pauseShortcuts,
    saveBinding,
    takePendingBinding,
  } from '../api';
  import { commandTargets } from '../bar/rows';
  import { captureShortcut, formatShortcut, isModifierKey } from '../keys';
  import Keys from '../components/Keys.vue';
  import { useWindowListener } from '../windowEvents';
  import type { Binding, Snapshot } from '../types';
  import InfoTip from '../components/InfoTip.vue';

  // Settings › Shortcuts: a global hotkey and/or a typed alias for any command, app, bookmark
  // or settings page. Apps, bookmarks and pages are added from the bar (Ctrl+K, Set hotkey or
  // alias…); commands can be added here too.
  const props = defineProps<{ snapshot: Snapshot }>();
  const emit = defineEmits<{ apply: [snapshot: Snapshot] }>();

  // Commands, plus each launch group.
  const COMMANDS = computed(() => [
    ...commandTargets(),
    ...(props.snapshot.launchGroups ?? []).map((group) => ({
      target: `group:${group.id}`,
      label: group.name,
    })),
  ]);
  /** One the bar asked about that isn't saved yet. */
  const draft = ref<Binding | null>(null);
  const listening = ref<string | null>(null);
  // Zephyr's own shortcuts are off while one is recorded, so a combo it uses records too.
  watch(
    () => listening.value !== null,
    (recording) => void pauseShortcuts(recording).catch(() => undefined)
  );
  const error = ref<{ target: string; message: string } | null>(null);
  const failures = ref<string[]>([]);
  const adding = ref('');
  const unlistens: UnlistenFn[] = [];

  const rows = computed<Binding[]>(() => {
    const saved = props.snapshot.bindings ?? [];
    const pending = draft.value;
    if (pending && !saved.some((binding) => binding.target === pending.target)) {
      return [pending, ...saved];
    }
    return saved;
  });

  function blank(target: string, label: string): Binding {
    return { target, label, hotkey: '', alias: '', direct: false };
  }

  function kindOf(target: string): string {
    if (target.startsWith('app:')) return 'App';
    if (target.startsWith('bookmark:')) return 'Bookmark';
    return 'Command';
  }

  async function takePending() {
    const pending = await takePendingBinding().catch(() => null);
    if (!pending) return;
    const saved = props.snapshot.bindings?.find((binding) => binding.target === pending.target);
    draft.value = saved ? null : blank(pending.target, pending.label);
    await nextTick();
    document
      .getElementById(`binding-${pending.target}`)
      ?.scrollIntoView({ block: 'center', behavior: 'smooth' });
  }

  async function refreshFailures() {
    failures.value = await bindingFailures().catch(() => []);
  }

  async function save(binding: Binding, change: Partial<Binding>) {
    const next = { ...binding, ...change };
    try {
      emit('apply', await saveBinding(next));
      error.value = null;
      if (draft.value?.target === binding.target) draft.value = null;
      await refreshFailures();
    } catch (err) {
      error.value = { target: binding.target, message: errorMessage(err) };
    }
  }

  function remove(binding: Binding) {
    if (draft.value?.target === binding.target) {
      draft.value = null;
      return;
    }
    void save(binding, { hotkey: '', alias: '' });
  }

  function add() {
    const command = COMMANDS.value.find((item) => item.target === adding.value);
    adding.value = '';
    if (!command) return;
    if (props.snapshot.bindings?.some((binding) => binding.target === command.target)) return;
    draft.value = blank(command.target, command.label);
  }

  /** While a hotkey is being recorded, the next full combo becomes that binding's hotkey. */
  function onKey(event: KeyboardEvent) {
    const target = listening.value;
    if (!target || event.isComposing) return;
    event.preventDefault();
    event.stopPropagation();
    const binding = rows.value.find((row) => row.target === target);
    if (event.key === 'Escape' || !binding) {
      listening.value = null;
      return;
    }
    const hotkey = captureShortcut(event, true);
    if (!hotkey) {
      if (!isModifierKey(event)) {
        error.value = { target, message: 'Use at least one modifier and a letter, number or key.' };
      }
      return;
    }
    listening.value = null;
    void save(binding, { hotkey });
  }

  useWindowListener('keydown', onKey, true);

  onMounted(async () => {
    unlistens.push(await listen('binding-pending', () => void takePending()));
    await takePending();
    await refreshFailures();
  });

  onUnmounted(() => {
    for (const unlisten of unlistens) unlisten();
  });
</script>

<template>
  <section id="section-shortcuts">
    <h2>
      Shortcuts
      <InfoTip
        text="A hotkey works from anywhere; an alias is typed in the bar. A command's alias then a space goes straight into that command, and a direct alias (for a symbol like =) fires as soon as it's typed. To add an app or bookmark, select it in the bar, press Ctrl+K and choose Set hotkey or alias."
      />
    </h2>

    <label class="field">
      <span>Add a command</span>
      <select v-model="adding" @change="add">
        <option value="" disabled>Choose a command…</option>
        <option v-for="command in COMMANDS" :key="command.target" :value="command.target">
          {{ command.label }}
        </option>
      </select>
    </label>

    <p v-if="!rows.length" class="hint-text">No shortcuts yet.</p>

    <div
      v-for="binding in rows"
      :id="`binding-${binding.target}`"
      :key="binding.target"
      class="binding"
    >
      <div class="binding-head">
        <strong>{{ binding.label }}</strong>
        <span class="hint-text">{{ kindOf(binding.target) }}</span>
        <button type="button" class="binding-remove" @click="remove(binding)">Remove</button>
      </div>
      <div class="binding-fields">
        <label class="field">
          <span>Hotkey</span>
          <span class="binding-hotkey">
            <button
              type="button"
              class="recorder"
              :class="{ listening: listening === binding.target }"
              @click="listening = binding.target"
              @blur="listening === binding.target && (listening = null)"
            >
              <template v-if="listening === binding.target">Press a shortcut</template>
              <template v-else-if="!binding.hotkey">None</template>
              <Keys v-else :keys="binding.hotkey" />
            </button>
            <button
              v-if="binding.hotkey"
              type="button"
              title="Clear the hotkey"
              @click="save(binding, { hotkey: '' })"
            >
              Clear
            </button>
          </span>
        </label>
        <label class="field">
          <span>Alias</span>
          <input
            :value="binding.alias"
            placeholder="None"
            spellcheck="false"
            autocomplete="off"
            @change="save(binding, { alias: ($event.target as HTMLInputElement).value })"
          />
        </label>
        <label class="check">
          <input
            type="checkbox"
            :checked="binding.direct"
            :disabled="!binding.alias"
            @change="save(binding, { direct: !binding.direct })"
          />
          Direct: fire as soon as the alias is typed
        </label>
      </div>
      <p v-if="failures.includes(binding.target)" class="notice">
        Another app already uses {{ formatShortcut(binding.hotkey) }}, so it does nothing right now.
        Pick another.
      </p>
      <p v-if="error?.target === binding.target" class="notice">{{ error.message }}</p>
    </div>
  </section>
</template>
