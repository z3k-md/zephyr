<script setup lang="ts">
  import type { SyncPending } from '../types';
  import { PLATFORM_NAMES } from '../format';
  import { MOD_KEY as mod } from '../keys';

  defineProps<{ device: SyncPending }>();
  defineEmits<{ answer: ['allow' | 'deny'] }>();
</script>

<template>
  <section class="approval" role="alert" aria-label="A device wants to sync">
    <header class="approval-head">
      <span class="approval-badge">⇄</span>
      <span class="approval-summary">
        Allow <strong>{{ device.name }}</strong> ({{
          PLATFORM_NAMES[device.platform] ?? device.platform
        }}) to sync?
      </span>
    </header>
    <p class="approval-words">Check it shows: {{ device.words }}</p>
    <div class="approval-actions">
      <button type="button" class="primary" @mousedown.prevent @click="$emit('answer', 'allow')">
        Allow <kbd>{{ mod }}Y</kbd>
      </button>
      <button type="button" @mousedown.prevent @click="$emit('answer', 'deny')">
        Deny <kbd>{{ mod }}N</kbd>
      </button>
    </div>
  </section>
</template>
