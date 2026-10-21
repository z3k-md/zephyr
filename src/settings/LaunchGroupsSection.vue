<script setup lang="ts">
  import { computed, onMounted, ref } from 'vue';
  import { errorMessage, installedApps, pickFolder, saveLaunchGroups } from '../api';
  import type { GroupItem, LaunchGroup, Snapshot } from '../types';

  // Settings › Launch groups: named sets of apps (and links and folders) the bar opens in
  // one go. Each group is a row in the bar by its name; Settings › Shortcuts gives it a
  // hotkey or an alias.
  const props = defineProps<{ snapshot: Snapshot }>();
  const emit = defineEmits<{ apply: [snapshot: Snapshot] }>();

  const ICONS = [
    { id: '', label: "First app's icon" },
    { id: 'terminal', label: 'Terminal' },
    { id: 'gamepad', label: 'Game controller' },
    { id: 'sparkle', label: 'Sparkle' },
    { id: 'folder', label: 'Folder' },
    { id: 'music', label: 'Music' },
    { id: 'grid', label: 'Grid' },
  ];

  const apps = ref<[string, string][]>([]);
  const error = ref<string | null>(null);
  const busy = ref(false);
  /** The text in each group's "Add app" box, by group id. */
  const adding = ref<Record<string, string>>({});
  const addingUrl = ref<Record<string, string>>({});

  const groups = computed(() => props.snapshot.launchGroups ?? []);

  onMounted(async () => {
    apps.value = await installedApps().catch(() => []);
  });

  // Edits build on the last one sent, not the last one saved, and save in order, so a rename
  // followed at once by an icon change keeps both.
  let draft: LaunchGroup[] | null = null;
  let saving: Promise<void> = Promise.resolve();

  function save(next: LaunchGroup[]) {
    draft = next;
    busy.value = true;
    error.value = null;
    saving = saving
      .then(async () => emit('apply', await saveLaunchGroups(next)))
      .catch((err) => {
        error.value = errorMessage(err);
      })
      .finally(() => {
        if (draft === next) {
          draft = null;
          busy.value = false;
        }
      });
  }

  function update(id: string, change: (group: LaunchGroup) => LaunchGroup) {
    save((draft ?? groups.value).map((group) => (group.id === id ? change(group) : group)));
  }

  function addGroup() {
    const id = `group-${Date.now().toString(36)}`;
    const current = draft ?? groups.value;
    save([...current, { id, name: `Group ${current.length + 1}`, icon: '', items: [] }]);
  }

  function remove(id: string) {
    save((draft ?? groups.value).filter((group) => group.id !== id));
  }

  function addItem(id: string, item: GroupItem) {
    update(id, (group) => ({ ...group, items: [...group.items, item] }));
  }

  function addApp(id: string) {
    const typed = (adding.value[id] ?? '').trim().toLowerCase();
    const found = apps.value.find(([, name]) => name.toLowerCase() === typed);
    if (!found) {
      error.value = typed ? `No installed app is called ${adding.value[id]}` : null;
      return;
    }
    adding.value[id] = '';
    addItem(id, { kind: 'app', value: found[0], label: found[1] });
  }

  function addUrl(id: string) {
    const url = (addingUrl.value[id] ?? '').trim();
    if (!url) return;
    const value = /^https?:\/\//.test(url) ? url : `https://${url}`;
    addingUrl.value[id] = '';
    addItem(id, { kind: 'url', value, label: value.replace(/^https?:\/\//, '') });
  }

  async function addFolder(id: string) {
    const folder = await pickFolder().catch(() => null);
    if (!folder) return;
    addItem(id, { kind: 'folder', value: folder, label: folder.split(/[\\/]/).pop() || folder });
  }

  function move(id: string, index: number, delta: number) {
    update(id, (group) => {
      const items = [...group.items];
      const [item] = items.splice(index, 1);
      items.splice(Math.max(0, Math.min(items.length, index + delta)), 0, item);
      return { ...group, items };
    });
  }

  function removeItem(id: string, index: number) {
    update(id, (group) => ({ ...group, items: group.items.filter((_, at) => at !== index) }));
  }
</script>

<template>
  <section id="section-launchgroups">
    <h2>Launch groups</h2>
    <p class="lede">
      Open several apps at once: type a group's name in the bar, or give it a hotkey in Shortcuts.
      Apps already open come to the front.
    </p>
    <p v-if="error" class="notice">{{ error }}</p>
    <article v-for="group in groups" :key="group.id" class="destination launch-group">
      <div class="destination-row">
        <input
          :value="group.name"
          type="text"
          maxlength="40"
          aria-label="Group name"
          @change="
            update(group.id, (g) => ({ ...g, name: ($event.target as HTMLInputElement).value }))
          "
        />
        <select
          :value="group.icon"
          aria-label="Icon"
          @change="
            update(group.id, (g) => ({ ...g, icon: ($event.target as HTMLSelectElement).value }))
          "
        >
          <option v-for="icon in ICONS" :key="icon.id" :value="icon.id">{{ icon.label }}</option>
        </select>
        <button type="button" :disabled="busy" @click="remove(group.id)">Delete</button>
      </div>
      <ul class="folders">
        <li v-for="(item, index) in group.items" :key="index">
          <span class="path">
            {{ item.label }}
            <span class="hint-text">{{ item.kind === 'app' ? '' : item.kind }}</span>
          </span>
          <button type="button" :disabled="busy || index === 0" @click="move(group.id, index, -1)">
            Up
          </button>
          <button
            type="button"
            :disabled="busy || index === group.items.length - 1"
            @click="move(group.id, index, 1)"
          >
            Down
          </button>
          <button type="button" :disabled="busy" @click="removeItem(group.id, index)">
            Remove
          </button>
        </li>
      </ul>
      <form class="field inline" @submit.prevent="addApp(group.id)">
        <input
          v-model="adding[group.id]"
          type="text"
          list="launch-group-apps"
          placeholder="Add an app"
          spellcheck="false"
        />
        <button type="submit" :disabled="busy">Add app</button>
      </form>
      <form class="field inline" @submit.prevent="addUrl(group.id)">
        <input
          v-model="addingUrl[group.id]"
          type="text"
          placeholder="Add a link"
          spellcheck="false"
        />
        <button type="submit" :disabled="busy">Add link</button>
        <button type="button" :disabled="busy" @click="addFolder(group.id)">Add folder…</button>
      </form>
    </article>
    <datalist id="launch-group-apps">
      <option v-for="[id, name] in apps" :key="id" :value="name" />
    </datalist>
    <div class="field inline">
      <button type="button" class="primary" :disabled="busy" @click="addGroup">New group</button>
    </div>
  </section>
</template>
