<script setup lang="ts">
  import { nextTick, onMounted, ref } from 'vue';
  import { aiAsk, aiCancel, errorMessage, hideBar } from '../api';

  const emit = defineEmits<{ exit: [] }>();

  interface Turn {
    question: string;
    answer: string;
    status: 'waiting' | 'streaming' | 'done' | 'error';
    error: string | null;
    source: string | null;
  }

  const isMac = navigator.userAgent.includes('Mac');
  const mod = isMac ? '⌘' : 'Ctrl+';

  const inputEl = ref<HTMLTextAreaElement | null>(null);
  const scroller = ref<HTMLElement | null>(null);
  const text = ref('');
  const turns = ref<Turn[]>([]);
  const notice = ref<string | null>(null);
  let generation = 0;

  onMounted(() => void focus());

  async function focus() {
    await nextTick();
    inputEl.value?.focus();
  }

  /** Asks `question`, continuing the conversation unless `fresh`. */
  async function ask(question: string, fresh = false) {
    const value = question.trim();
    if (!value) return;
    if (fresh) {
      generation++;
      void aiCancel().catch(() => undefined);
      turns.value = [];
    }
    const history = turns.value
      .filter((turn) => turn.status === 'done')
      .map((turn) => ({ question: turn.question, answer: turn.answer }));
    const turn: Turn = {
      question: value,
      answer: '',
      status: 'waiting',
      error: null,
      source: null,
    };
    turns.value.push(turn);
    const index = turns.value.length - 1;
    const mine = ++generation;
    text.value = '';
    notice.value = null;
    void follow();
    const current = () => (mine === generation ? turns.value[index] : null);
    try {
      await aiAsk(
        value,
        (event) => {
          const target = current();
          if (!target) return;
          if (event.kind === 'started') target.source = `${event.model} · ${event.provider}`;
          else if (event.kind === 'delta') {
            target.answer += event.text;
            target.status = 'streaming';
            void follow();
          } else if (event.kind === 'done') target.status = 'done';
          else {
            target.status = 'error';
            target.error = event.message;
          }
        },
        history
      );
    } catch (error) {
      const target = current();
      if (target) {
        target.status = 'error';
        target.error = errorMessage(error);
      }
    }
  }

  // Keep the newest text in view unless the user scrolled up to read.
  async function follow() {
    const el = scroller.value;
    const pinned = !el || el.scrollHeight - el.scrollTop - el.clientHeight < 24;
    await nextTick();
    if (pinned && scroller.value) scroller.value.scrollTop = scroller.value.scrollHeight;
  }

  async function copyLast() {
    const last = [...turns.value].reverse().find((turn) => turn.answer);
    if (!last) return;
    try {
      await navigator.clipboard.writeText(last.answer.trim());
      await hideBar();
    } catch {
      notice.value = "Couldn't copy the answer";
    }
  }

  /** Called by the bar for every key while Ask AI is showing. */
  function onKey(event: KeyboardEvent) {
    const modifier = event.metaKey || event.ctrlKey;
    if (event.key === 'Escape') {
      event.preventDefault();
      emit('exit');
      return;
    }
    if (event.key === 'Backspace' && !event.repeat && text.value === '') {
      event.preventDefault();
      emit('exit');
      return;
    }
    if (modifier && event.shiftKey && event.code === 'KeyC') {
      event.preventDefault();
      void copyLast();
      return;
    }
    if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) {
      event.preventDefault();
      if (event.repeat) return;
      // ⌘↵ starts a fresh conversation; plain Enter follows up, or copies the answer when
      // there's nothing typed.
      if (!text.value.trim() && !modifier) {
        void copyLast();
        return;
      }
      void ask(text.value, modifier);
    }
  }

  defineExpose({ onKey, ask, focus });
</script>

<template>
  <div class="ai-view">
    <div ref="scroller" class="ai-turns" aria-live="polite">
      <article v-for="(turn, index) in turns" :key="index" class="ai-turn">
        <p class="ai-question">{{ turn.question }}</p>
        <p v-if="turn.status === 'waiting'" class="answer-wait">Thinking…</p>
        <!-- prettier-ignore -->
        <div v-else-if="turn.answer" class="answer-text">{{ turn.answer }}<span v-if="turn.status === 'streaming'" class="caret" /></div>
        <p v-if="turn.error" class="answer-error">{{ turn.error }}</p>
        <p v-if="turn.source && index === turns.length - 1" class="answer-source">
          {{ turn.source }}
        </p>
      </article>
      <p v-if="!turns.length" class="empty">Ask anything. Follow-ups keep the conversation.</p>
    </div>
    <p v-if="notice" class="notice">{{ notice }}</p>
    <div class="input-row ai-input">
      <span class="ai-mark" aria-hidden="true">✦</span>
      <textarea
        ref="inputEl"
        v-model="text"
        class="query"
        rows="1"
        :placeholder="turns.length ? 'Follow up…' : 'Ask AI'"
        spellcheck="false"
      />
    </div>
    <footer class="footer">
      <span>
        ↵ {{ text.trim() ? (turns.length ? 'follow up' : 'ask') : 'copy answer' }} · {{ mod }}↵ new
        conversation · ⇧↵ new line · {{ mod }}⇧C copy · Esc back
      </span>
      <span>Ask AI</span>
    </footer>
  </div>
</template>
