<script setup lang="ts">
  import type { SyncPending } from '../types';

  defineProps<{ device: SyncPending }>();
  defineEmits<{ answer: ['allow' | 'deny'] }>();

  const mod = navigator.userAgent.includes('Mac') ? '⌘' : 'Ctrl+';
  const PLATFORMS: Record<string, string> = { macos: 'Mac', windows: 'Windows', linux: 'Linux' };
</script>

<template>
  <section class="approval" role="alert" aria-label="A device wants to sync">
    <header class="approval-head">
      <span class="approval-badge">⇄</span>
      <span class="approval-summary">
        Allow <strong>{{ device.name }}</strong> ({{
          PLATFORMS[device.platform] ?? device.platform
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
