<script setup lang="ts">
  import { computed } from 'vue';
  import type { Suggestion } from '../types';
  import RowIcon from './RowIcon.vue';
  import KeyHints from '../components/KeyHints.vue';

  // "port 3000": the process with the port open, as the list's first row. Enter shows it
  // in Resources; a quit waiting for its second Enter says so here.
  const props = defineProps<{
    row: Suggestion;
    selected: boolean;
    /** "Quit node?" while a quit waits for Enter. */
    confirm: string;
  }>();

  const use = computed(() => props.row.portUse);
  const facts = computed(() => {
    const item = use.value;
    if (!item) return '';
    return [`PID ${item.pid}`, item.protocol, item.local, item.remote ? `→ ${item.remote}` : '']
      .filter(Boolean)
      .join(' · ');
  });

  const hints = computed(() =>
    props.confirm
      ? [
          { keys: 'enter', label: 'yes' },
          { keys: 'esc', label: 'no' },
        ]
      : [
          { keys: 'enter', label: 'Resources' },
          { keys: 'mod+k', label: 'Actions' },
        ]
  );
</script>

<template>
  <section v-if="use" class="port-card" :class="{ selected }" aria-label="Port">
    <RowIcon :icon="use.icon" :label="row.label" :size="32" />
    <div class="port-card-text">
      <div class="net-card-head">
        <span class="net-pill" :class="{ good: use.state === 'LISTEN' }">
          {{ use.state || use.protocol }}
        </span>
        <span class="net-card-caption">Port {{ use.port }}</span>
        <KeyHints v-if="selected || confirm" :hints="hints" />
      </div>
      <div class="net-card-big">{{ confirm || row.label }}</div>
      <div class="net-card-sub">{{ facts }}</div>
      <div v-if="use.command" class="port-card-command" :title="use.command">
        {{ use.command }}
      </div>
    </div>
  </section>
</template>
