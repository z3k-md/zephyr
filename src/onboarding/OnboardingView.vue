<script setup lang="ts">
  import '../settings/settings.css';
  import { nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import {
    errorMessage,
    finishOnboarding,
    getSnapshot,
    pageReady,
    pauseShortcuts,
    saveSettings,
    saveTheme,
  } from '../api';
  import { GLYPHS } from '../bar/glyphs';
  import { THEMES } from '../themes';
  import BootChoice from '../settings/BootChoice.vue';
  import Keys from '../components/Keys.vue';
  import { captureShortcut, heldModifiers, isModifierKey, SHORTCUT_HELP } from '../keys';
  import { isSnapshot, type Snapshot } from '../types';
  import { useWindowListener } from '../windowEvents';

  // First-run setup: three quick choices, each already set to a good default, so any step
  // can be skipped. Finishing or skipping marks it done for good.
  const STEPS = ['Shortcut', 'Theme', 'Startup'] as const;
  const last = STEPS.length - 1;
  const step = ref(0);
  /** Which way the last move went, for the slide. */
  const direction = ref<'forward' | 'back'>('forward');
  const snapshot = ref<Snapshot | null>(null);
  const error = ref<string | null>(null);
  const busy = ref(false);
  const listening = ref(false);
  /** The modifiers held while recording, shown as they're pressed. */
  const held = ref('');
  // Zephyr's own shortcuts are off while one is recorded, so the current one records too.
  watch(listening, (recording) => {
    held.value = '';
    void pauseShortcuts(recording).catch(() => undefined);
  });
  /** Esc once asks; Esc again (or the button) skips. */
  const confirmSkip = ref(false);
  const nextButton = ref<HTMLButtonElement | null>(null);
  let unlisten: UnlistenFn | null = null;

  useWindowListener('keydown', onKey, true);
  useWindowListener('keyup', onKeyUp, true);

  onMounted(async () => {
    try {
      snapshot.value = await getSnapshot();
    } catch (err) {
      error.value = errorMessage(err);
    }
    pageReady();
    unlisten = await listen<unknown>('state-changed', (event) => {
      if (isSnapshot(event.payload)) snapshot.value = event.payload;
    });
  });

  onUnmounted(() => unlisten?.());

  function onKeyUp(event: KeyboardEvent) {
    if (listening.value) held.value = heldModifiers(event);
  }

  async function run(task: () => Promise<Snapshot | void>) {
    busy.value = true;
    error.value = null;
    try {
      const next = await task();
      if (next) snapshot.value = next;
    } catch (err) {
      error.value = errorMessage(err);
    } finally {
      busy.value = false;
    }
  }

  function save(shortcut: string, startup: boolean) {
    return run(() => saveSettings(shortcut, startup));
  }

  function go(to: number) {
    if (to < 0 || to > last || to === step.value) return;
    direction.value = to > step.value ? 'forward' : 'back';
    step.value = to;
    confirmSkip.value = false;
    listening.value = false;
    error.value = null;
    void nextTick(() => nextButton.value?.focus());
  }

  function next() {
    if (step.value < last) go(step.value + 1);
    else void finish();
  }

  async function finish() {
    await run(() => finishOnboarding());
  }

  /** This window takes the theme at once, so the choice shows while it's being made. */
  function chooseTheme(id: string) {
    document.documentElement.dataset.theme = id;
    void run(() => saveTheme(id));
  }

  function recordShortcut(event: KeyboardEvent) {
    const current = snapshot.value;
    if (!current) return;
    if (event.key === 'Escape') {
      listening.value = false;
      return;
    }
    if (isModifierKey(event)) return;
    const shortcut = captureShortcut(event);
    if (!shortcut) {
      error.value = SHORTCUT_HELP;
      return;
    }
    listening.value = false;
    void save(shortcut, current.launchAtStartup);
  }

  // Enter goes on, ⇧Enter or ⌘[ goes back, Esc asks to skip. A shortcut being recorded
  // takes every key first.
  function onKey(event: KeyboardEvent) {
    if (event.isComposing) return;
    if (listening.value) {
      event.preventDefault();
      event.stopPropagation();
      held.value = heldModifiers(event);
      recordShortcut(event);
      return;
    }
    if (event.key === 'Escape') {
      event.preventDefault();
      if (confirmSkip.value) void finish();
      else confirmSkip.value = true;
      return;
    }
    confirmSkip.value = false;
    const back = (event.key === 'Enter' && event.shiftKey) || (event.metaKey && event.key === '[');
    if (back) {
      event.preventDefault();
      go(step.value - 1);
      return;
    }
    const onControl = (event.target as HTMLElement | null)?.closest('button, a, input');
    if (event.key === 'Enter' && !event.shiftKey && !onControl) {
      event.preventDefault();
      next();
    }
  }
</script>

<template>
  <main v-if="snapshot" class="settings onboarding">
    <!-- The step train: done steps are links back. -->
    <nav class="train" aria-label="Setup steps">
      <ol>
        <li
          v-for="(label, index) in STEPS"
          :key="label"
          :class="{ current: index === step, done: index < step }"
        >
          <button
            type="button"
            :disabled="index > step"
            :aria-current="index === step ? 'step' : undefined"
            @click="go(index)"
          >
            <span class="train-dot" aria-hidden="true">
              <svg v-if="index < step" viewBox="0 0 24 24" v-html="GLYPHS.check" />
              <template v-else>{{ index + 1 }}</template>
            </span>
            {{ label }}
          </button>
        </li>
      </ol>
    </nav>

    <div class="onboarding-body">
      <Transition :name="`slide-${direction}`" mode="out-in">
        <section :key="step" class="onboarding-step">
          <template v-if="step === 0">
            <h1>Choose a hotkey</h1>
            <button
              type="button"
              class="recorder big"
              :class="{ listening }"
              :disabled="busy"
              @click="listening = true"
              @blur="listening = false"
            >
              <template v-if="listening && held">
                <Keys :keys="held" />
                <span class="recorder-more">…</span>
              </template>
              <template v-else-if="listening">Press a shortcut…</template>
              <Keys v-else :keys="snapshot.summonShortcut" />
            </button>
          </template>

          <template v-else-if="step === 1">
            <h1>Choose a theme</h1>
            <div class="themes compact" role="radiogroup" aria-label="Theme">
              <button
                v-for="theme in THEMES"
                :key="theme.id"
                type="button"
                class="theme"
                role="radio"
                :aria-checked="snapshot.theme === theme.id"
                :class="{ chosen: snapshot.theme === theme.id }"
                :disabled="busy"
                @click="chooseTheme(theme.id)"
              >
                <span class="theme-swatch" :style="{ background: theme.tint }" aria-hidden="true">
                  <span class="theme-row" />
                  <span class="theme-row selected" :style="{ background: theme.accent }" />
                  <span class="theme-row" />
                </span>
                {{ theme.name }}
              </button>
            </div>
          </template>

          <template v-else>
            <h1>Run at boot</h1>
            <BootChoice :snapshot="snapshot" @apply="(next) => (snapshot = next)" />
          </template>

          <p v-if="error" class="notice">{{ error }}</p>
        </section>
      </Transition>
    </div>

    <footer class="onboarding-foot">
      <button type="button" class="link-button" :disabled="busy" @click="finish">
        <template v-if="confirmSkip">Press <Keys keys="esc" small /> again to skip setup</template>
        <template v-else>Skip setup</template>
      </button>
      <span class="spacer" />
      <button v-if="step > 0" type="button" :disabled="busy" @click="go(step - 1)">Back</button>
      <button ref="nextButton" type="button" class="primary" :disabled="busy" @click="next">
        {{ step === last ? 'Finish' : 'Next' }}
      </button>
    </footer>
  </main>
</template>
