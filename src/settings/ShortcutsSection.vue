<script setup lang="ts">
  import { computed, nextTick, onMounted, onUnmounted, ref } from 'vue';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import {
    bindingFailures,
    errorMessage,
    formatShortcut,
    saveBinding,
    takePendingBinding,
  } from '../api';
  import { commandTargets } from '../bar/rows';
  import type { Binding, Snapshot } from '../types';

  // Settings › Shortcuts: a global hotkey and/or a typed alias for any command, app,
  // destination or bookmark. Apps, destinations and bookmarks are added from the bar (Ctrl+K,
  // Set hotkey or alias…); commands can be added here too.
  const props = defineProps<{ snapshot: Snapshot }>();
  const emit = defineEmits<{ apply: [snapshot: Snapshot] }>();

  const COMMANDS = commandTargets();
  /** One the bar asked about that isn't saved yet. */
  const draft = ref<Binding | null>(null);
  const listening = ref<string | null>(null);
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
    if (target.startsWith('dest:')) return 'Destination';
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
    const command = COMMANDS.find((item) => item.target === adding.value);
    adding.value = '';
    if (!command) return;
    if (props.snapshot.bindings?.some((binding) => binding.target === command.target)) return;
    draft.value = blank(command.target, command.label);
  }

  /** The keys pressed, as a shortcut string, or null until there's a modifier and a key. */
  function captured(event: KeyboardEvent): string | null {
    const parts: string[] = [];
    if (event.ctrlKey) parts.push('ctrl');
    if (event.altKey) parts.push('alt');
    if (event.shiftKey) parts.push('shift');
    if (event.metaKey) parts.push('super');
    if (parts.length === 0) return null;
    if (['Control', 'Alt', 'Shift', 'Meta'].includes(event.key)) return null;
    if (event.key === ' ' || event.key === 'Spacebar') parts.push('space');
    else if (/^F\d{1,2}$/.test(event.key)) parts.push(event.key.toLowerCase());
    else if (event.key.length === 1) parts.push(event.key.toLowerCase());
    else return null;
    return parts.join('+');
  }

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
    const hotkey = captured(event);
    if (!hotkey) {
      if (!['Control', 'Alt', 'Shift', 'Meta'].includes(event.key)) {
        error.value = { target, message: 'Use at least one modifier and a letter, number or key.' };
      }
      return;
    }
    listening.value = null;
    void save(binding, { hotkey });
  }

  onMounted(async () => {
    window.addEventListener('keydown', onKey, true);
    unlistens.push(await listen('binding-pending', () => void takePending()));
    await takePending();
    await refreshFailures();
  });

  onUnmounted(() => {
    window.removeEventListener('keydown', onKey, true);
    for (const unlisten of unlistens) unlisten();
  });
</script>

<template>
  <section id="section-shortcuts">
    <h2>Shortcuts</h2>
    <p class="lede">
      Give a command, app, destination or bookmark a hotkey that works from anywhere, or an alias
      you type in the bar. An alias then a space goes straight into that destination or command; a
      direct alias (for a symbol like <code>=</code>) fires as soon as it's typed. To add an app,
      destination or bookmark, select it in the bar, press Ctrl+K and choose Set hotkey or alias.
    </p>

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
            >
              {{
                listening === binding.target
                  ? 'Press a shortcut'
                  : binding.hotkey
                    ? formatShortcut(binding.hotkey)
                    : 'None'
              }}
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
