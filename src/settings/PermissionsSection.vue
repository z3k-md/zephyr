<script setup lang="ts">
  import { onMounted, onUnmounted, ref, watch } from 'vue';
  import { errorMessage, permissionRequest, permissionsStatus } from '../api';
  import type { Permission, PermissionId } from '../types';
  import InfoTip from '../components/InfoTip.vue';

  // Settings › Permissions (macOS): what the app may do, why it wants each, and an Allow that
  // asks in place. Status is re-read every second while the page is open, so a switch turned
  // on in System Settings shows here at once.
  const props = defineProps<{ active: boolean }>();

  const ABOUT: Record<PermissionId, { name: string; why: string; settings: boolean }> = {
    accessibility: {
      name: 'Accessibility',
      why: 'Paste from clipboard history straight into the app you are using.',
      settings: true,
    },
    automation: {
      name: 'Automation (System Events)',
      why: 'Shut down, restart or log out when you ask from the bar.',
      settings: false,
    },
    files: {
      name: 'Desktop, Documents and Downloads',
      why: 'Find files in these folders from the bar. Nothing leaves your Mac.',
      settings: false,
    },
  };

  const permissions = ref<Permission[]>([]);
  const waiting = ref<PermissionId | null>(null);
  const error = ref<string | null>(null);
  let timer = 0;

  async function refresh() {
    try {
      permissions.value = await permissionsStatus();
      if (waiting.value && status(waiting.value) === 'granted') waiting.value = null;
    } catch (err) {
      error.value = errorMessage(err);
    }
  }

  function status(id: PermissionId) {
    return permissions.value.find((permission) => permission.id === id)?.status;
  }

  async function allow(id: PermissionId) {
    error.value = null;
    waiting.value = id;
    try {
      permissions.value = await permissionRequest(id);
      if (status(id) === 'granted') waiting.value = null;
    } catch (err) {
      error.value = errorMessage(err);
      waiting.value = null;
    }
  }

  function label(permission: Permission): string {
    if (permission.status === 'denied') return 'Open Settings';
    return ABOUT[permission.id].settings ? 'Allow in Settings' : 'Allow';
  }

  function poll(on: boolean) {
    window.clearInterval(timer);
    if (on) {
      void refresh();
      // Closing Settings only hides it, so the section can stay "open" all session.
      timer = window.setInterval(() => {
        if (!document.hidden) void refresh();
      }, 1000);
    }
  }

  onMounted(() => poll(props.active));
  watch(
    () => props.active,
    (on) => poll(on)
  );
  onUnmounted(() => window.clearInterval(timer));
</script>

<template>
  <section id="section-permissions">
    <h2>
      Permissions
      <InfoTip text="Zephyr asks for each of these the first time a feature needs it." />
    </h2>
    <p v-if="error" class="notice">{{ error }}</p>
    <ul class="permissions">
      <li v-for="permission in permissions" :key="permission.id" class="permission">
        <span
          class="permission-dot"
          :class="permission.status"
          :title="permission.status === 'granted' ? 'Allowed' : 'Not allowed'"
        />
        <span class="permission-text">
          <strong>
            {{ ABOUT[permission.id].name }}
            <InfoTip :text="ABOUT[permission.id].why" />
          </strong>
          <span v-if="waiting === permission.id && ABOUT[permission.id].settings" class="lede">
            Turn on Zephyr in the list that opened. This updates on its own.
          </span>
        </span>
        <span v-if="permission.status === 'granted'" class="hint-text">Allowed</span>
        <button v-else type="button" @click="allow(permission.id)">
          {{ label(permission) }}
        </button>
      </li>
    </ul>
  </section>
</template>
