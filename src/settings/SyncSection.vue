<script setup lang="ts">
  import { computed, onMounted, onUnmounted, ref } from 'vue';
  import {
    errorMessage,
    syncCancelSignIn,
    syncPoll,
    syncRecoveryKey,
    syncRecoverySaved,
    syncGoogleSignIn,
    syncRevoke,
    syncSignOut,
    syncStatus,
    syncUseRecovery,
  } from '../api';
  import type { SyncStatus } from '../types';

  const status = ref<SyncStatus | null>(null);
  const recoveryInput = ref('');
  const shownRecovery = ref<string | null>(null);
  const savedConfirm = ref(false);
  const busy = ref(false);
  const error = ref<string | null>(null);
  const useRecovery = ref(false);
  const confirmSignOut = ref(false);

  let poller = 0;

  const phase = computed(() => status.value?.phase.kind ?? 'signedOut');
  const others = computed(() =>
    (status.value?.devices ?? []).filter(
      (device) => device.id !== status.value?.deviceId && !device.revokedAt
    )
  );

  onMounted(async () => {
    await refresh();
    // While waiting for another device's approval, check every 5 seconds.
    poller = window.setInterval(() => {
      if (phase.value === 'awaitingApproval')
        void syncPoll()
          .then(refresh)
          .catch(() => undefined);
    }, 5000);
  });

  onUnmounted(() => window.clearInterval(poller));

  async function refresh() {
    try {
      status.value = await syncStatus();
    } catch (err) {
      error.value = errorMessage(err);
    }
  }

  async function act(task: () => Promise<unknown>) {
    busy.value = true;
    error.value = null;
    try {
      await task();
      await refresh();
    } catch (err) {
      error.value = errorMessage(err);
    } finally {
      busy.value = false;
    }
  }

  function signIn() {
    // Show "finish in your browser" right away; the call resolves when the browser returns.
    void refresh().then(() => {
      if (status.value) status.value = { ...status.value, phase: { kind: 'browser' } };
    });
    void act(syncGoogleSignIn);
  }

  function finishSetup() {
    void act(syncRecoverySaved);
  }

  function unlockWithRecovery() {
    void act(async () => {
      await syncUseRecovery(recoveryInput.value);
      recoveryInput.value = '';
      useRecovery.value = false;
    });
  }

  function showRecovery() {
    void act(async () => {
      shownRecovery.value = await syncRecoveryKey();
    });
  }

  function remove(id: string) {
    void act(() => syncRevoke(id));
  }

  function signOut() {
    if (!confirmSignOut.value) {
      confirmSignOut.value = true;
      return;
    }
    confirmSignOut.value = false;
    shownRecovery.value = null;
    void act(syncSignOut);
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      error.value = "Couldn't copy it";
    }
  }

  function seen(when: string | null): string {
    if (!when) return 'never';
    const seconds = (Date.now() - new Date(when).getTime()) / 1000;
    if (seconds < 120) return 'just now';
    if (seconds < 3600) return `${Math.floor(seconds / 60)} min ago`;
    if (seconds < 86400) return `${Math.floor(seconds / 3600)} h ago`;
    return new Date(when).toLocaleDateString();
  }

  const PLATFORMS: Record<string, string> = { macos: 'Mac', windows: 'Windows', linux: 'Linux' };
</script>

<template>
  <section id="section-sync">
    <h2>Sync</h2>
    <p class="lede">Notes and clipboard, end-to-end encrypted.</p>
    <p v-if="error" class="notice">{{ error }}</p>

    <template v-if="phase === 'signedOut'">
      <button type="button" class="primary google-button" :disabled="busy" @click="signIn">
        Sign in with Google
      </button>
    </template>

    <template v-else-if="phase === 'browser'">
      <p class="hint-text">Finish signing in in your browser…</p>
      <button type="button" @click="act(syncCancelSignIn)">Cancel</button>
    </template>

    <template v-else-if="status?.phase.kind === 'showRecovery'">
      <h3>Save your recovery key</h3>
      <p class="lede">You'll need it to set up a computer when no other device can approve it.</p>
      <div class="recovery-key">
        <code>{{ status.phase.recovery }}</code>
        <button type="button" @click="copy(status.phase.recovery)">Copy</button>
      </div>
      <label class="check">
        <input v-model="savedConfirm" type="checkbox" />
        I saved my recovery key somewhere safe
      </label>
      <button type="button" class="primary" :disabled="busy || !savedConfirm" @click="finishSetup">
        Finish setup
      </button>
    </template>

    <template v-else-if="status?.phase.kind === 'awaitingApproval'">
      <h3>Approve this computer</h3>
      <p class="lede">Allow it from a computer that already syncs. Both show these words:</p>
      <p class="device-words">{{ status.phase.words }}</p>
      <p class="hint-text">Waiting for approval…</p>
      <button v-if="!useRecovery" type="button" @click="useRecovery = true">
        Use recovery key instead
      </button>
      <form v-else class="field inline" @submit.prevent="unlockWithRecovery">
        <input v-model="recoveryInput" type="text" placeholder="XXXX-XXXX-…" spellcheck="false" />
        <button type="submit" class="primary" :disabled="busy || !recoveryInput.trim()">
          Unlock
        </button>
      </form>
      <button type="button" :disabled="busy" @click="signOut">
        {{ confirmSignOut ? 'Click again to sign out' : 'Sign out' }}
      </button>
    </template>

    <template v-else-if="phase === 'ready' && status">
      <p class="hint-text">
        Signed in as {{ status.email }} · this computer is “{{ status.deviceName }}” ·
        {{ status.words }}
      </p>
      <h3>Your devices</h3>
      <ul class="folders">
        <li>
          <span class="path">
            {{ status.deviceName }} ·
            {{
              PLATFORMS[status.devices.find((d) => d.id === status?.deviceId)?.platform ?? ''] ?? ''
            }}
            · this computer
          </span>
        </li>
        <li v-for="device in others" :key="device.id">
          <span class="path">
            {{ device.displayName }} · {{ PLATFORMS[device.platform] ?? device.platform }} · seen
            {{ seen(device.lastSeenAt) }}
          </span>
          <button type="button" :disabled="busy" @click="remove(device.id)">Remove</button>
        </li>
      </ul>
      <div class="field inline">
        <button type="button" :disabled="busy" @click="showRecovery">Show recovery key</button>
        <button type="button" :disabled="busy" @click="signOut">
          {{ confirmSignOut ? 'Click again to sign out' : 'Sign out' }}
        </button>
      </div>
      <div v-if="shownRecovery" class="recovery-key">
        <code>{{ shownRecovery }}</code>
        <button type="button" @click="copy(shownRecovery)">Copy</button>
      </div>
    </template>
  </section>
</template>
