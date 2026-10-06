<script setup lang="ts">
  import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
  import { errorMessage, saveTypingResult } from '../api';
  import type { TypingBest } from '../types';
  import { consistency, tally, wpm } from './stats';
  import { randomWords } from './words';

  const props = defineProps<{ bests: TypingBest[] }>();
  const emit = defineEmits<{ exit: [] }>();

  type Kind = 'time' | 'words';
  interface Mode {
    kind: Kind;
    amount: number;
  }
  const MODES: Mode[] = [
    { kind: 'time', amount: 15 },
    { kind: 'time', amount: 30 },
    { kind: 'time', amount: 60 },
    { kind: 'words', amount: 10 },
    { kind: 'words', amount: 25 },
    { kind: 'words', amount: 50 },
  ];

  interface Result {
    wpm: number;
    raw: number;
    accuracy: number;
    consistency: number;
    seconds: number;
    correct: number;
    incorrect: number;
    extra: number;
    missed: number;
    samples: number[];
    best: boolean;
  }

  const mode = ref<Mode>(MODES[1]);
  const words = ref<string[]>([]);
  const typed = ref<string[]>(['']);
  const index = ref(0);
  const startedAt = ref<number | null>(null);
  const now = ref(Date.now());
  const result = ref<Result | null>(null);
  const notice = ref<string | null>(null);
  const wordsEl = ref<HTMLElement | null>(null);
  const caret = ref({ left: 0, top: 0 });
  const offset = ref(0);
  const blink = ref(true);

  let keystrokes = 0;
  let correctKeystrokes = 0;
  let ticker = 0;
  /** Characters typed so far at the end of each second, for the speed chart. */
  let perSecond: { all: number; correct: number }[] = [];

  const modeKey = computed(() => `${mode.value.kind}-${mode.value.amount}`);
  const best = computed(() => props.bests.find((item) => item.mode === modeKey.value));
  const elapsed = computed(() => (startedAt.value ? now.value - startedAt.value : 0));
  const remaining = computed(() =>
    mode.value.kind === 'time'
      ? Math.max(0, Math.ceil(mode.value.amount - elapsed.value / 1000))
      : null
  );
  const progress = computed(() =>
    mode.value.kind === 'time'
      ? `${remaining.value}`
      : `${Math.min(index.value, mode.value.amount)}/${mode.value.amount}`
  );
  const liveWpm = computed(() => {
    if (!startedAt.value || elapsed.value < 1000) return 0;
    const score = tally(words.value, typed.value, index.value);
    return Math.round(wpm(score.correctWordChars, elapsed.value));
  });

  onMounted(() => restart());

  onBeforeUnmount(() => window.clearInterval(ticker));

  watch([typed, index], () => void nextTick(placeCaret), { deep: true });

  function restart(next?: Mode) {
    if (next) mode.value = next;
    window.clearInterval(ticker);
    words.value = randomWords(mode.value.kind === 'words' ? mode.value.amount : 120);
    typed.value = [''];
    index.value = 0;
    startedAt.value = null;
    result.value = null;
    notice.value = null;
    offset.value = 0;
    keystrokes = 0;
    correctKeystrokes = 0;
    perSecond = [];
    blink.value = true;
    void nextTick(placeCaret);
  }

  function start() {
    startedAt.value = Date.now();
    now.value = startedAt.value;
    blink.value = false;
    ticker = window.setInterval(tick, 100);
  }

  function tick() {
    if (!startedAt.value) return;
    now.value = Date.now();
    const second = Math.floor(elapsed.value / 1000);
    while (perSecond.length < second) {
      const score = tally(words.value, typed.value, index.value);
      perSecond.push({ all: score.allChars, correct: score.correctWordChars });
    }
    if (mode.value.kind === 'time' && elapsed.value >= mode.value.amount * 1000) finish();
  }

  async function finish() {
    if (!startedAt.value || result.value) return;
    window.clearInterval(ticker);
    const ms =
      mode.value.kind === 'time'
        ? mode.value.amount * 1000
        : Math.max(1, Date.now() - startedAt.value);
    const finished = mode.value.kind === 'words' ? words.value.length : index.value;
    const score = tally(words.value, typed.value, finished);
    const raws = perSecond.map((sample, at) => {
      const before = at === 0 ? 0 : perSecond[at - 1].all;
      return wpm(sample.all - before, 1000);
    });
    const samples = perSecond.map((sample, at) => wpm(sample.correct, (at + 1) * 1000));
    const outcome: Result = {
      wpm: wpm(score.correctWordChars, ms),
      raw: wpm(score.allChars, ms),
      accuracy: keystrokes ? (correctKeystrokes / keystrokes) * 100 : 0,
      consistency: consistency(raws),
      seconds: ms / 1000,
      correct: score.correct,
      incorrect: score.incorrect,
      extra: score.extra,
      missed: score.missed,
      samples,
      best: false,
    };
    result.value = outcome;
    try {
      outcome.best = await saveTypingResult({
        mode: modeKey.value,
        wpm: outcome.wpm,
        raw: outcome.raw,
        accuracy: outcome.accuracy,
        consistency: outcome.consistency,
        at: Math.floor(Date.now() / 1000),
      });
    } catch (error) {
      notice.value = errorMessage(error);
    }
  }

  /** Positions are offsets within the lines block, which scrolling never changes. */
  function placeCaret() {
    const container = wordsEl.value;
    if (!container) return;
    const word = container.querySelector<HTMLElement>(`[data-word="${index.value}"]`);
    if (!word) return;
    const letters = word.querySelectorAll<HTMLElement>('.letter');
    const at = typed.value[index.value]?.length ?? 0;
    let left = word.offsetLeft;
    let top = word.offsetTop;
    if (at < letters.length) {
      left = letters[at].offsetLeft;
      top = letters[at].offsetTop;
    } else if (letters.length) {
      const last = letters[letters.length - 1];
      left = last.offsetLeft + last.offsetWidth;
      top = last.offsetTop;
    }
    // Keep the current line as the second of three visible lines.
    const lineHeight = word.offsetHeight || 32;
    offset.value = Math.max(0, word.offsetTop - lineHeight);
    caret.value = { left, top };
  }

  /** Called by the bar for every key while the typing test is showing. */
  function onKey(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault();
      emit('exit');
      return;
    }
    if (event.key === 'Tab') {
      event.preventDefault();
      restart();
      return;
    }
    if (result.value) {
      if (event.key === 'Enter') {
        event.preventDefault();
        restart();
      }
      return;
    }
    if (event.metaKey || (event.ctrlKey && event.key !== 'Backspace')) return;

    if (event.key === 'Backspace') {
      event.preventDefault();
      backspace(event.altKey || event.ctrlKey || event.metaKey);
      return;
    }
    if (event.key === ' ') {
      event.preventDefault();
      space();
      return;
    }
    if (event.key.length !== 1) return;
    event.preventDefault();
    if (!startedAt.value) start();
    const current = typed.value[index.value] ?? '';
    const target = words.value[index.value] ?? '';
    // Like Monkeytype, a word can run at most a few characters long.
    if (current.length >= target.length + 10) return;
    keystrokes++;
    if (event.key === target[current.length]) correctKeystrokes++;
    typed.value[index.value] = current + event.key;
    const last = index.value === words.value.length - 1;
    if (last && typed.value[index.value] === target) {
      index.value++;
      void finish();
    }
  }

  function space() {
    const current = typed.value[index.value] ?? '';
    if (!current) return;
    if (!startedAt.value) start();
    keystrokes++;
    if (current === words.value[index.value]) correctKeystrokes++;
    index.value++;
    if (index.value >= words.value.length) {
      if (mode.value.kind === 'words') {
        void finish();
        return;
      }
      words.value.push(...randomWords(60));
    }
    typed.value[index.value] = '';
  }

  function backspace(wholeWord: boolean) {
    const current = typed.value[index.value] ?? '';
    if (current) {
      typed.value[index.value] = wholeWord ? '' : current.slice(0, -1);
      return;
    }
    // Back into the previous word only when it has a mistake to fix.
    if (index.value > 0 && typed.value[index.value - 1] !== words.value[index.value - 1]) {
      typed.value.pop();
      index.value--;
      if (wholeWord) typed.value[index.value] = '';
    }
  }

  function letterClass(wordIndex: number, at: number): string {
    const input = typed.value[wordIndex];
    if (input === undefined || at >= input.length) {
      const skipped = wordIndex < index.value;
      return skipped ? 'letter missed' : 'letter';
    }
    return input[at] === words.value[wordIndex][at] ? 'letter correct' : 'letter wrong';
  }

  function extras(wordIndex: number): string {
    const input = typed.value[wordIndex] ?? '';
    return input.slice(words.value[wordIndex].length);
  }

  const chart = computed(() => {
    const samples = result.value?.samples ?? [];
    if (samples.length < 2) return '';
    const top = Math.max(...samples, 1);
    return samples
      .map((value, at) => {
        const x = (at / (samples.length - 1)) * 300;
        const y = 60 - (value / top) * 56;
        return `${x.toFixed(1)},${y.toFixed(1)}`;
      })
      .join(' ');
  });

  function label(item: Mode): string {
    return item.kind === 'time' ? `${item.amount}s` : `${item.amount} words`;
  }

  defineExpose({ onKey });
</script>

<template>
  <div class="typing">
    <div class="typing-modes">
      <button
        v-for="item in MODES"
        :key="`${item.kind}-${item.amount}`"
        type="button"
        class="chip"
        :class="{ active: item.kind === mode.kind && item.amount === mode.amount }"
        @mousedown.prevent
        @click="restart(item)"
      >
        {{ label(item) }}
      </button>
      <span class="typing-best">
        {{ best ? `Best ${Math.round(best.wpm)} wpm` : 'No best yet' }}
      </span>
    </div>

    <template v-if="!result">
      <div class="typing-live">
        <span class="typing-progress">{{ progress }}</span>
        <span v-if="startedAt" class="typing-wpm">{{ liveWpm }} wpm</span>
      </div>
      <div ref="wordsEl" class="typing-words" aria-label="Words to type">
        <div class="typing-lines" :style="{ transform: `translateY(${-offset}px)` }">
          <span
            v-for="(word, wordIndex) in words"
            :key="wordIndex"
            :data-word="wordIndex"
            class="word"
            :class="{
              error:
                wordIndex < index && typed[wordIndex] !== undefined && typed[wordIndex] !== word,
            }"
          >
            <span v-for="(char, at) in word" :key="at" :class="letterClass(wordIndex, at)">{{
              char
            }}</span
            ><span v-if="extras(wordIndex)" class="letter extra">{{ extras(wordIndex) }}</span>
          </span>
        </div>
        <span
          class="typing-caret"
          :class="{ blink }"
          :style="{ transform: `translate(${caret.left}px, ${caret.top - offset}px)` }"
        />
      </div>
      <p class="typing-hint">
        {{ startedAt ? 'Tab restarts · Esc exits' : 'Start typing · Tab restarts · Esc exits' }}
      </p>
    </template>

    <section v-else class="typing-result">
      <div class="typing-big">
        <div>
          <span class="typing-label">wpm</span>
          <strong>{{ Math.round(result.wpm) }}</strong>
        </div>
        <div>
          <span class="typing-label">acc</span>
          <strong>{{ Math.round(result.accuracy) }}%</strong>
        </div>
        <span v-if="result.best" class="typing-pb">New personal best</span>
      </div>
      <svg v-if="chart" class="typing-chart" viewBox="0 0 300 62" preserveAspectRatio="none">
        <polyline :points="chart" />
      </svg>
      <dl class="typing-stats">
        <div>
          <dt>raw</dt>
          <dd>{{ Math.round(result.raw) }}</dd>
        </div>
        <div>
          <dt>consistency</dt>
          <dd>{{ Math.round(result.consistency) }}%</dd>
        </div>
        <div>
          <dt>characters</dt>
          <dd :title="'correct / incorrect / extra / missed'">
            {{ result.correct }}/{{ result.incorrect }}/{{ result.extra }}/{{ result.missed }}
          </dd>
        </div>
        <div>
          <dt>time</dt>
          <dd>{{ Math.round(result.seconds) }}s</dd>
        </div>
      </dl>
      <p v-if="notice" class="notice">{{ notice }}</p>
      <p class="typing-hint">Enter or Tab for another · Esc exits</p>
    </section>
  </div>
</template>
