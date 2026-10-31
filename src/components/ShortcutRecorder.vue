<script setup lang="ts">
  import { computed, onUnmounted, ref, watch } from 'vue';
  import { pauseShortcuts } from '../api';
  import { captureShortcut, heldModifiers, isModifierKey, SHORTCUT_HELP } from '../keys';
  import { useWindowListener } from '../windowEvents';
  import Keys from './Keys.vue';

  // One field for recording a shortcut, shared by every hotkey in Settings. A click starts
  // recording (the field turns red) and it keeps recording until another click, a click
  // away or Enter: the keys show as they go down, and
  // each new combo replaces the last, so a slip is fixed by pressing again. Esc leaves it as
  // it was, and Zephyr's own shortcuts are off while it records so they record too.
  const props = withDefaults(
    defineProps<{
      /** The shortcut as saved ("alt+g"), or empty for none. */
      keys: string;
      /** Shown when there's none. */
      empty?: string;
      /** F1–F12 count as keys. */
      functionKeys?: boolean;
      /** Backspace sets none. */
      clearable?: boolean;
      disabled?: boolean;
    }>(),
    { empty: 'No shortcut set', functionKeys: false, clearable: false, disabled: false }
  );
  const emit = defineEmits<{ record: [shortcut: string]; invalid: [message: string] }>();

  const listening = ref(false);
  /** What's down right now: modifiers so far ("ctrl+alt"), or a whole combo. */
  const live = ref('');
  /** The last whole combo pressed while recording; saved on Stop. */
  const candidate = ref('');
  const snapped = ref(false);
  let snapTimer = 0;

  watch(listening, (on) => {
    void pauseShortcuts(on).catch(() => undefined);
    live.value = '';
    candidate.value = '';
  });
  onUnmounted(() => {
    window.clearTimeout(snapTimer);
    if (listening.value) void pauseShortcuts(false).catch(() => undefined);
  });

  /** What the field shows: the keys going down, the last combo pressed, or what's saved. */
  const shown = computed(() =>
    listening.value ? live.value || candidate.value || props.keys : props.keys
  );
  /** Modifiers held with the key still to come. */
  const waiting = computed(() => listening.value && !!live.value && live.value !== candidate.value);

  function toggle() {
    if (props.disabled) return;
    if (listening.value) stop();
    else listening.value = true;
  }

  /** Ends recording, saving the last combo pressed if there was one. */
  function stop() {
    if (!listening.value) return;
    const shortcut = candidate.value;
    if (shortcut && shortcut !== props.keys) commit(shortcut);
    else listening.value = false;
  }

  function onKeyDown(event: KeyboardEvent) {
    if (!listening.value || event.isComposing) return;
    event.preventDefault();
    event.stopPropagation();
    if (event.repeat) return;
    const modifiers = heldModifiers(event);
    if (event.key === 'Escape' && !modifiers) {
      listening.value = false;
      return;
    }
    if (event.key === 'Enter' && !modifiers) {
      stop();
      return;
    }
    if (props.clearable && event.key === 'Backspace' && !modifiers) {
      commit('');
      return;
    }
    if (isModifierKey(event)) {
      live.value = modifiers;
      return;
    }
    const shortcut = captureShortcut(event, props.functionKeys);
    if (!shortcut) {
      emit('invalid', SHORTCUT_HELP);
      return;
    }
    live.value = shortcut;
    candidate.value = shortcut;
  }

  // Letting go keeps the combo on show until every key is up; letting go of a modifier
  // before any key just takes it back off the field.
  function onKeyUp(event: KeyboardEvent) {
    if (!listening.value) return;
    event.preventDefault();
    event.stopPropagation();
    const modifiers = heldModifiers(event);
    if (!modifiers) live.value = '';
    else if (live.value !== candidate.value) live.value = modifiers;
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
    @click="toggle"
    @blur="stop"
  >
    <span v-if="shown" class="recorder-keys">
      <Keys :keys="shown" />
      <kbd v-if="waiting" class="kbd recorder-waiting" aria-hidden="true">+</kbd>
    </span>
    <span v-else-if="listening" class="recorder-prompt">Press a shortcut</span>
    <span v-else class="recorder-empty">{{ empty }}</span>
  </button>
</template>
