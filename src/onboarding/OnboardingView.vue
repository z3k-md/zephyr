<script setup lang="ts">
  import { computed, nextTick, onMounted, onUnmounted, ref } from 'vue';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import {
    chatgptAccount,
    chatgptSignIn,
    errorMessage,
    finishOnboarding,
    formatShortcut,
    getSnapshot,
    openSettings,
    pageReady,
    saveAiChoice,
    saveSettings,
    saveTheme,
  } from '../api';
  import { askAiName, chosenAi, isAiLike } from '../aiChoice';
  import { GLYPHS } from '../bar/glyphs';
  import { THEMES } from '../themes';
  import { captureShortcut } from '../keys';
  import { isSnapshot, type ChatgptAccount, type Snapshot } from '../types';

  // First-run setup: five quick choices, each already set to a good default, so any step
  // can be skipped. Finishing or skipping marks it done for good.
  const STEPS = [
    { label: 'Shortcut', icon: 'keyboard' },
    { label: 'Search', icon: 'search' },
    { label: 'AI', icon: 'sparkle' },
    { label: 'Theme', icon: 'palette' },
    { label: 'Done', icon: 'check' },
  ] as const;
  const last = STEPS.length - 1;
  const step = ref(0);
  /** Which way the last move went, for the slide. */
  const direction = ref<'forward' | 'back'>('forward');
  const snapshot = ref<Snapshot | null>(null);
  const error = ref<string | null>(null);
  const busy = ref(false);
  const listening = ref(false);
  const chatgpt = ref<ChatgptAccount | null>(null);
  const signingIn = ref(false);
  /** Esc once asks; Esc again (or the button) skips. */
  const confirmSkip = ref(false);
  const isMac = navigator.userAgent.includes('Mac');
  const nextButton = ref<HTMLButtonElement | null>(null);
  let unlisten: UnlistenFn | null = null;

  const searchOptions = computed(() =>
    (snapshot.value?.destinations ?? []).filter(
      (destination) => !destination.disabled && !isAiLike(destination)
    )
  );
  const aiOptions = computed(() =>
    (snapshot.value?.destinations ?? []).filter(
      (destination) => !destination.disabled && isAiLike(destination)
    )
  );
  const aiId = computed(() => chosenAi(snapshot.value)?.id ?? '');

  onMounted(async () => {
    try {
      snapshot.value = await getSnapshot();
    } catch (err) {
      error.value = errorMessage(err);
    }
    chatgpt.value = await chatgptAccount().catch(() => null);
    pageReady();
    unlisten = await listen<unknown>('state-changed', (event) => {
      if (isSnapshot(event.payload)) snapshot.value = event.payload;
    });
    window.addEventListener('keydown', onKey, true);
  });

  onUnmounted(() => {
    unlisten?.();
    window.removeEventListener('keydown', onKey, true);
  });

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

  function save(shortcut: string, startup: boolean, destination: string) {
    return run(() => saveSettings(shortcut, startup, destination));
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

  function recordShortcut(event: KeyboardEvent) {
    const current = snapshot.value;
    if (!current) return;
    if (event.key === 'Escape') {
      listening.value = false;
      return;
    }
    if (['Control', 'Alt', 'Shift', 'Meta'].includes(event.key)) return;
    const shortcut = captureShortcut(event);
    if (!shortcut) {
      error.value = 'Use at least one modifier and a letter, number, or space.';
      return;
    }
    listening.value = false;
    void save(shortcut, current.launchAtStartup, current.defaultDestinationId);
  }

  // Enter goes on, ⇧Enter or ⌘[ goes back, Esc asks to skip. A shortcut being recorded
  // takes every key first.
  function onKey(event: KeyboardEvent) {
    if (event.isComposing) return;
    if (listening.value) {
      event.preventDefault();
      event.stopPropagation();
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

  async function signIn() {
    signingIn.value = true;
    error.value = null;
    try {
      chatgpt.value = await chatgptSignIn();
      snapshot.value = await getSnapshot();
    } catch (err) {
      error.value = errorMessage(err);
    } finally {
      signingIn.value = false;
    }
  }
</script>

<template>
  <main v-if="snapshot" class="settings onboarding">
    <!-- The step train: done steps are links back. -->
    <nav class="train" aria-label="Setup steps">
      <ol>
        <li
          v-for="(item, index) in STEPS"
          :key="item.label"
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
            {{ item.label }}
          </button>
        </li>
      </ol>
    </nav>

    <div class="onboarding-body">
      <Transition :name="`slide-${direction}`" mode="out-in">
        <section :key="step" class="onboarding-step">
          <span class="onboarding-icon" aria-hidden="true">
            <svg viewBox="0 0 24 24" v-html="GLYPHS[STEPS[step].icon]" />
          </span>

          <template v-if="step === 0">
            <h1>Welcome to Zephyr</h1>
            <p class="lede">One shortcut opens the bar from anywhere.</p>
            <button
              type="button"
              class="recorder big"
              :class="{ listening }"
              :disabled="busy"
              @click="listening = true"
            >
              {{ listening ? 'Press a shortcut…' : formatShortcut(snapshot.summonShortcut) }}
            </button>
            <p class="hint-text">Click to change it, or keep this one.</p>
          </template>

          <template v-else-if="step === 1">
            <h1>Where searches go</h1>
            <p class="lede">Text you type goes here. Tab switches for a single search.</p>
            <div class="choices" role="radiogroup" aria-label="Search">
              <button
                v-for="destination in searchOptions"
                :key="destination.id"
                type="button"
                role="radio"
                class="choice"
                :aria-checked="snapshot.defaultDestinationId === destination.id"
                :class="{ chosen: snapshot.defaultDestinationId === destination.id }"
                :disabled="busy"
                @click="save(snapshot.summonShortcut, snapshot.launchAtStartup, destination.id)"
              >
                {{ destination.name }}
              </button>
            </div>
          </template>

          <template v-else-if="step === 2">
            <h1>Your AI</h1>
            <p class="lede">
              One AI sits in the bar, on Tab and {{ isMac ? '⌘↵' : 'Ctrl+Enter' }}.
            </p>
            <div class="choices" role="radiogroup" aria-label="AI">
              <button
                v-for="option in aiOptions"
                :key="option.id"
                type="button"
                role="radio"
                class="choice"
                :aria-checked="aiId === option.id"
                :class="{ chosen: aiId === option.id }"
                :disabled="busy"
                @click="run(() => saveAiChoice(option.id))"
              >
                {{ option.kind === 'ai' ? `${askAiName(snapshot)} (in Zephyr)` : option.name }}
              </button>
            </div>
            <div v-if="aiId === 'ai'" class="onboarding-ai">
              <p v-if="chatgpt?.plan" class="hint-text">
                Answering with your ChatGPT plan ({{ chatgpt.email }}).
              </p>
              <template v-else>
                <button
                  v-if="!signingIn"
                  type="button"
                  class="chatgpt-button"
                  :disabled="busy"
                  @click="signIn"
                >
                  Continue with ChatGPT
                </button>
                <span v-else class="hint-text">Finish signing in in your browser…</span>
                <p class="hint-text">
                  Or an API key or local model, in
                  <a href="#" @click.prevent="openSettings">Settings › AI</a>.
                </p>
              </template>
            </div>
          </template>

          <template v-else-if="step === 3">
            <h1>Pick a look</h1>
            <p class="lede">Every Zephyr window follows it, right away.</p>
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
                @click="run(() => saveTheme(theme.id))"
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
            <h1>You're set</h1>
            <p class="lede">
              Press {{ formatShortcut(snapshot.summonShortcut) }} and type: an app, a search, a
              question, or "settings".
            </p>
            <label class="check">
              <input
                type="checkbox"
                :checked="snapshot.launchAtStartup"
                :disabled="busy"
                @change="
                  save(
                    snapshot.summonShortcut,
                    !snapshot.launchAtStartup,
                    snapshot.defaultDestinationId
                  )
                "
              />
              Start Zephyr when you sign in
            </label>
          </template>

          <p v-if="error" class="notice">{{ error }}</p>
        </section>
      </Transition>
    </div>

    <footer class="onboarding-foot">
      <button type="button" class="link-button" :disabled="busy" @click="finish">
        {{ confirmSkip ? 'Press Esc again to skip setup' : 'Skip setup' }}
      </button>
      <span class="spacer" />
      <button v-if="step > 0" type="button" :disabled="busy" @click="go(step - 1)">Back</button>
      <button ref="nextButton" type="button" class="primary" :disabled="busy" @click="next">
        {{ step === last ? 'Finish' : 'Next' }}
      </button>
    </footer>
  </main>
</template>
