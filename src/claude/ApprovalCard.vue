<script setup lang="ts">
  import type { ClaudeApproval } from '../types';
  import { MOD_KEY as mod } from '../keys';

  defineProps<{ approval: ClaudeApproval; queued: number }>();
  defineEmits<{ answer: ['allow' | 'always' | 'deny'] }>();
</script>

<template>
  <section class="approval" role="alert" aria-label="Claude needs permission">
    <header class="approval-head">
      <span class="approval-badge">?</span>
      <strong>{{ approval.project }}</strong>
      <span class="approval-summary">{{ approval.summary }}</span>
      <span v-if="queued > 1" class="hint">{{ queued - 1 }} more waiting</span>
    </header>
    <pre v-if="approval.detail" class="approval-detail">{{ approval.detail }}</pre>
    <div class="approval-actions">
      <button type="button" class="primary" @mousedown.prevent @click="$emit('answer', 'allow')">
        Allow once <kbd>{{ mod }}Y</kbd>
      </button>
      <button type="button" @mousedown.prevent @click="$emit('answer', 'always')">
        Always in this project <kbd>{{ mod }}⇧Y</kbd>
      </button>
      <button type="button" @mousedown.prevent @click="$emit('answer', 'deny')">
        Deny <kbd>{{ mod }}N</kbd>
      </button>
      <span class="hint">Type a reason first to tell Claude why</span>
    </div>
  </section>
</template>
