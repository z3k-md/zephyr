<script setup lang="ts">
  import { ref } from 'vue';
  import { errorMessage, saveInstantStartup } from '../api';
  import type { Snapshot } from '../types';

  // Windows only: start from a Task Scheduler logon task instead of the startup list, which
  // Windows holds back for a few seconds after sign-in. Opt-in, with the Defender caveat said
  // up front, since Defender quarantined Zephyr for it once.
  const props = defineProps<{ snapshot: Snapshot }>();
  const emit = defineEmits<{ apply: [snapshot: Snapshot] }>();

  const isWindows = navigator.userAgent.includes('Windows');
  const busy = ref(false);
  const error = ref<string | null>(null);

  async function toggle() {
    busy.value = true;
    try {
      emit('apply', await saveInstantStartup(!props.snapshot.instantStartup));
      error.value = null;
    } catch (err) {
      error.value = errorMessage(err);
    } finally {
      busy.value = false;
    }
  }
</script>

<template>
  <div v-if="isWindows" class="instant-startup">
    <label class="check">
      <input
        type="checkbox"
        :checked="snapshot.instantStartup"
        :disabled="busy || !snapshot.launchAtStartup"
        @change="toggle"
      />
      Start instantly at sign-in
    </label>
    <p class="hint-text">
      Windows waits a few seconds after you sign in before starting apps, so the shortcut can miss
      at first. This starts Zephyr right away from a Task Scheduler task instead. Windows Defender
      may flag that as suspicious (Behavior:Win32/Persistence) and quarantine Zephyr. If it does,
      open Windows Security › Virus &amp; threat protection › Protection history, choose the Zephyr
      entry and Allow it, then start Zephyr again. Or leave this off.
    </p>
    <p v-if="error" class="notice">{{ error }}</p>
  </div>
</template>
