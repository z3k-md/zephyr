<script setup lang="ts">
  import { computed, onUnmounted, ref, watch } from 'vue';
  import { pauseShortcuts } from '../api';
  import { captureShortcut, heldModifiers, isModifierKey, SHORTCUT_HELP } from '../keys';
  import { useWindowListener } from '../windowEvents';
  import Keys from './Keys.vue';

  // One field for recording a shortcut, shared by every hotkey in Settings. A click starts
  // listening (the field pulses); the keys show as they go down, modifiers first with a "+"
  // waiting for the key; letting go binds it with a small snap. Esc or a click away leaves
  // it as it was, and Zephyr's own shortcuts are off while it listens so they record too.
  const props = withDefaults(
    defineProps<{
      /** The shortcut as saved ("alt+g"), or empty for none. */
      keys: string;
      /** Shown when there's none ("Add hotkey", "None"). */
      empty?: string;
      /** F1–F12 count as keys. */
      functionKeys?: boolean;
      /** Backspace sets none. */
      clearable?: boolean;
      disabled?: boolean;
    }>(),
    { empty: 'None', functionKeys: false, clearable: false, disabled: false }
  );
  const emit = defineEmits<{ record: [shortcut: string]; invalid: [message: string] }>();

  const listening = ref(false);
  /** Modifiers held so far ("ctrl+alt"), while listening. */
  const held = ref('');
  /** The whole combo, down but not yet let go. */
  const pressed = ref('');
  const snapped = ref(false);
  let snapTimer = 0;

  watch(listening, (on) => {
    void pauseShortcuts(on).catch(() => undefined);
    held.value = '';
    pressed.value = '';
  });
  onUnmounted(() => {
    window.clearTimeout(snapTimer);
    if (listening.value) void pauseShortcuts(false).catch(() => undefined);
  });

  /** What the field shows: the combo going down, the modifiers so far, or what's saved. */
  const shown = computed(() => (listening.value ? pressed.value || held.value : props.keys));

  function start() {
    if (!props.disabled) listening.value = true;
  }

  function onKeyDown(event: KeyboardEvent) {
    if (!listening.value || event.isComposing) return;
    event.preventDefault();
    event.stopPropagation();
    if (event.repeat) return;
    if (event.key === 'Escape') {
      listening.value = false;
      return;
    }
    if (props.clearable && event.key === 'Backspace' && !heldModifiers(event)) {
      commit('');
      return;
    }
    if (isModifierKey(event)) {
      held.value = heldModifiers(event);
      return;
    }
    const shortcut = captureShortcut(event, props.functionKeys);
    if (!shortcut) {
      emit('invalid', SHORTCUT_HELP);
      return;
    }
    pressed.value = shortcut;
  }

  // Letting go of the key binds what was pressed; letting go of a modifier before any key
  // just takes it back off the field.
  function onKeyUp(event: KeyboardEvent) {
    if (!listening.value) return;
    event.preventDefault();
    event.stopPropagation();
    if (pressed.value) {
      commit(pressed.value);
      return;
    }
    held.value = heldModifiers(event);
  }

  function commit(shortcut: string) {
    listening.value = false;
    emit('record', shortcut);
    snapped.value = true;
    window.clearTimeout(snapTimer);
    snapTimer = window.setTimeout(() => (snapped.value = false), 320);
  }

  useWindowListener('keydown', onKeyDown, true);
  useWindowListener('keyup', onKeyUp, true);
</script>

<template>
  <button
    type="button"
    class="recorder"
    :class="{ listening, snapped }"
    :disabled="disabled"
    :aria-pressed="listening"
    @click="start"
    @blur="listening = false"
  >
    <span v-if="shown" class="recorder-keys">
      <Keys :keys="shown" />
      <kbd v-if="listening && !pressed" class="kbd recorder-waiting" aria-hidden="true">+</kbd>
    </span>
    <span v-else-if="listening" class="recorder-prompt">Press a shortcut</span>
    <span v-else class="recorder-empty">{{ empty }}</span>
  </button>
</template>
