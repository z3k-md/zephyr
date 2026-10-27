<script setup lang="ts">
  import type { SyncPending } from '../types';
  import { PLATFORM_NAMES } from '../format';
  import Keys from '../components/Keys.vue';

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
        Allow <Keys keys="mod+y" small />
      </button>
      <button type="button" @mousedown.prevent @click="$emit('answer', 'deny')">
        Deny <Keys keys="mod+n" small />
      </button>
    </div>
  </section>
</template>
