<script setup lang="ts">
  import { nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { EditorContent, useEditor } from '@tiptap/vue-3';
  import StarterKit from '@tiptap/starter-kit';
  import { FontFamily, FontSize, TextStyle } from '@tiptap/extension-text-style';
  import Highlight from '@tiptap/extension-highlight';
  import { TaskItem, TaskList } from '@tiptap/extension-list';
  import { Placeholder } from '@tiptap/extensions';
  import {
    dispatch,
    errorMessage,
    leaveNotes,
    noteCreate,
    noteDelete,
    noteGet,
    noteSave,
    notesList,
    pageReady,
    revealNotes,
  } from '../api';
  import type { NoteSummary } from '../types';
  import { renderMarkdown } from './markdown';

  const isMac = navigator.userAgent.includes('Mac');
  const mod = isMac ? '⌘' : 'Ctrl+';

  const FONTS = [
    { label: 'System', value: '' },
    { label: 'Serif', value: 'ui-serif, Georgia, serif' },
    { label: 'Mono', value: "ui-monospace, 'SF Mono', Menlo, monospace" },
    { label: 'Rounded', value: "ui-rounded, 'SF Pro Rounded', system-ui, sans-serif" },
  ];
  const SIZES = ['', '12px', '14px', '16px', '18px', '22px', '28px', '36px'];
  const HIGHLIGHTS = ['#fde68a66', '#86efac55', '#93c5fd55', '#f9a8d455', '#c4b5fd66'];

  const noteId = ref<string | null>(null);
  const browsing = ref(false);
  const search = ref('');
  const notes = ref<NoteSummary[]>([]);
  const browseIndex = ref(0);
  const status = ref<string | null>(null);
  const confirmDelete = ref(false);
  const searchEl = ref<HTMLInputElement | null>(null);
  const words = ref(0);
  // Toolbar state follows the cursor.
  const font = ref('');
  const size = ref('');
  const block = ref('p');

  let saveTimer = 0;
  let savedBody = '';
  let loading = false;
  let unlisten: UnlistenFn | null = null;

  const editor = useEditor({
    extensions: [
      StarterKit.configure({
        link: { openOnClick: false, autolink: true },
        heading: { levels: [1, 2, 3] },
      }),
      TextStyle,
      FontFamily,
      FontSize,
      Highlight.configure({ multicolor: true }),
      TaskList,
      TaskItem.configure({ nested: true }),
      Placeholder.configure({ placeholder: 'Start typing…' }),
    ],
    editorProps: {
      attributes: { class: 'notes-doc', spellcheck: 'true' },
      handleClickOn: (_view, _pos, _node, _nodePos, event) => {
        const link = (event.target as HTMLElement).closest('a[href]') as HTMLAnchorElement | null;
        if (link && (event.metaKey || event.ctrlKey)) {
          void dispatch(link.href, 'url', false).catch(() => undefined);
          return true;
        }
        return false;
      },
    },
    onUpdate: () => {
      if (loading) return;
      confirmDelete.value = false;
      countWords();
      window.clearTimeout(saveTimer);
      saveTimer = window.setTimeout(() => void flush(), 400);
    },
    onSelectionUpdate: () => syncToolbar(),
    onTransaction: () => syncToolbar(),
  });

  onMounted(async () => {
    pageReady();
    window.addEventListener('keydown', onKey, true);
    unlisten = await listen<string>('notes-open', (event) => {
      void openNote(event.payload);
    });
    const requested = new URLSearchParams(window.location.search).get('id');
    if (requested) {
      await openNote(requested);
    } else {
      const recent = await notesList('');
      if (recent.length) await openNote(recent[0].id);
      else await newNote();
    }
  });

  onBeforeUnmount(() => {
    window.removeEventListener('keydown', onKey, true);
    unlisten?.();
    void flush();
    editor.value?.destroy();
  });

  watch(search, () => {
    browseIndex.value = 0;
    void loadList();
  });

  function html(): string {
    const value = editor.value;
    if (!value || value.isEmpty) return '';
    return value.getHTML();
  }

  function countWords() {
    const text = editor.value?.getText().trim() ?? '';
    words.value = text ? text.split(/\s+/).length : 0;
  }

  function syncToolbar() {
    const value = editor.value;
    if (!value) return;
    const style = value.getAttributes('textStyle');
    font.value = (style.fontFamily as string | undefined) ?? '';
    size.value = (style.fontSize as string | undefined) ?? '';
    block.value = value.isActive('heading', { level: 1 })
      ? 'h1'
      : value.isActive('heading', { level: 2 })
        ? 'h2'
        : value.isActive('heading', { level: 3 })
          ? 'h3'
          : 'p';
  }

  function setContent(body: string) {
    loading = true;
    editor.value?.commands.setContent(body, { emitUpdate: false });
    loading = false;
    countWords();
    syncToolbar();
  }

  async function flush() {
    window.clearTimeout(saveTimer);
    const body = html();
    if (!noteId.value || body === savedBody) return;
    try {
      await noteSave(noteId.value, body);
      savedBody = body;
      status.value = null;
    } catch (error) {
      status.value = errorMessage(error);
    }
  }

  /** Leaves the current note, dropping it if nothing was ever written in it. */
  async function leave() {
    await flush();
    if (noteId.value && !html()) {
      await noteDelete(noteId.value).catch(() => undefined);
    }
  }

  async function focusEditor() {
    await nextTick();
    editor.value?.commands.focus('end');
  }

  async function openNote(id: string) {
    if (id === noteId.value) {
      browsing.value = false;
      await focusEditor();
      return;
    }
    await leave();
    try {
      const note = await noteGet(id);
      noteId.value = note.id;
      const markdown = note.format === 'markdown';
      savedBody = markdown ? '' : note.body;
      browsing.value = false;
      setContent(markdown ? renderMarkdown(note.body) : note.body);
      await focusEditor();
    } catch (error) {
      status.value = errorMessage(error);
    }
  }

  async function newNote() {
    if (noteId.value && !html()) {
      browsing.value = false;
      await focusEditor();
      return;
    }
    await leave();
    try {
      noteId.value = await noteCreate('');
      savedBody = '';
      browsing.value = false;
      setContent('');
      await focusEditor();
    } catch (error) {
      status.value = errorMessage(error);
    }
  }

  async function removeNote() {
    if (!noteId.value) return;
    if (!confirmDelete.value && html()) {
      confirmDelete.value = true;
      return;
    }
    confirmDelete.value = false;
    const id = noteId.value;
    noteId.value = null;
    await noteDelete(id).catch(() => undefined);
    const rest = await notesList('');
    if (rest.length) await openNote(rest[0].id);
    else await newNote();
  }

  async function copyText() {
    try {
      await navigator.clipboard.writeText(editor.value?.getText() ?? '');
      status.value = 'Copied the text';
      window.setTimeout(() => (status.value = null), 1500);
    } catch {
      status.value = "Couldn't copy the note";
    }
  }

  async function loadList() {
    try {
      notes.value = await notesList(search.value);
    } catch (error) {
      status.value = errorMessage(error);
    }
  }

  async function toggleBrowser() {
    browsing.value = !browsing.value;
    if (browsing.value) {
      await flush();
      search.value = '';
      await loadList();
      browseIndex.value = Math.max(
        0,
        notes.value.findIndex((note) => note.id === noteId.value)
      );
      await nextTick();
      searchEl.value?.focus();
    } else {
      await focusEditor();
    }
  }

  async function createFromSearch() {
    const text = search.value.trim();
    await leave();
    try {
      noteId.value = await noteCreate(text);
      const note = await noteGet(noteId.value);
      savedBody = note.body;
      browsing.value = false;
      setContent(note.body);
      await focusEditor();
    } catch (error) {
      status.value = errorMessage(error);
    }
  }

  // Toolbar

  function chain() {
    return editor.value!.chain().focus();
  }

  function setBlock(value: string) {
    if (value === 'p') chain().setParagraph().run();
    else
      chain()
        .toggleHeading({ level: Number(value.slice(1)) as 1 | 2 | 3 })
        .run();
  }

  function setFont(value: string) {
    if (value) chain().setFontFamily(value).run();
    else chain().unsetFontFamily().run();
  }

  function setSize(value: string) {
    if (value) chain().setFontSize(value).run();
    else chain().unsetFontSize().run();
  }

  function highlight(color: string | null) {
    if (color) chain().toggleHighlight({ color }).run();
    else chain().unsetHighlight().run();
  }

  function active(name: string): boolean {
    return editor.value?.isActive(name) ?? false;
  }

  function onKey(event: KeyboardEvent) {
    const modifier = event.metaKey || event.ctrlKey;
    if (modifier && event.code === 'KeyN') {
      event.preventDefault();
      void newNote();
      return;
    }
    if (modifier && event.code === 'KeyP') {
      event.preventDefault();
      void toggleBrowser();
      return;
    }
    if (modifier && event.shiftKey && event.code === 'KeyC') {
      event.preventDefault();
      void copyText();
      return;
    }
    if (modifier && event.shiftKey && event.code === 'KeyH') {
      event.preventDefault();
      highlight(HIGHLIGHTS[0]);
      return;
    }
    if (modifier && event.shiftKey && event.key === 'Backspace') {
      event.preventDefault();
      void removeNote();
      return;
    }
    // Esc leaves Notes as the sticky mode and returns to the bar.
    if (!browsing.value && event.key === 'Escape') {
      event.preventDefault();
      void flush().then(() => leaveNotes());
      return;
    }
    if (!browsing.value) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      void toggleBrowser();
    } else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      const count = notes.value.length;
      if (count) {
        browseIndex.value =
          (browseIndex.value + (event.key === 'ArrowDown' ? 1 : -1) + count) % count;
      }
    } else if (event.key === 'Enter') {
      event.preventDefault();
      const picked = notes.value[browseIndex.value];
      if (picked) void openNote(picked.id);
      else if (search.value.trim()) void createFromSearch();
    }
  }

  function ago(seconds: number): string {
    const diff = Math.max(0, Date.now() / 1000 - seconds);
    if (diff < 60) return 'just now';
    if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
    return new Date(seconds * 1000).toLocaleDateString();
  }
</script>

<template>
  <main class="notes">
    <header class="notes-bar">
      <button
        type="button"
        class="notes-title"
        :title="`All notes (${mod}P)`"
        @click="toggleBrowser"
      >
        {{ browsing ? 'All notes' : 'Notes' }}
        <span aria-hidden="true">▾</span>
      </button>
      <div class="notes-actions">
        <button type="button" :title="`New note (${mod}N)`" @click="newNote">New</button>
        <button type="button" :title="`Copy text (${mod}⇧C)`" @click="copyText">Copy</button>
        <button type="button" title="Show the notes folder" @click="revealNotes">Folder</button>
        <button
          type="button"
          :title="`Delete note (${mod}⇧⌫)`"
          :class="{ danger: confirmDelete }"
          @click="removeNote"
        >
          {{ confirmDelete ? 'Delete?' : 'Delete' }}
        </button>
      </div>
    </header>

    <section v-if="browsing" class="notes-browser">
      <input
        ref="searchEl"
        v-model="search"
        class="notes-search"
        type="text"
        placeholder="Search notes"
        spellcheck="false"
      />
      <ul class="notes-list">
        <li
          v-for="(note, index) in notes"
          :key="note.id"
          :class="{ selected: index === browseIndex, current: note.id === noteId }"
          @mousemove="browseIndex = index"
          @click="openNote(note.id)"
        >
          <strong>{{ note.title }}</strong>
          <span>{{ note.snippet || 'No more text' }}</span>
          <time>{{ ago(note.updated) }}</time>
        </li>
        <li v-if="!notes.length" class="notes-empty" @click="createFromSearch">
          {{ search.trim() ? `Enter to create “${search.trim()}”` : 'No notes yet' }}
        </li>
      </ul>
    </section>

    <template v-else>
      <div v-if="editor" class="notes-toolbar" @mousedown.prevent>
        <select
          :value="block"
          title="Text style"
          @mousedown.stop
          @change="setBlock(($event.target as HTMLSelectElement).value)"
        >
          <option value="p">Body</option>
          <option value="h1">Title</option>
          <option value="h2">Heading</option>
          <option value="h3">Subheading</option>
        </select>
        <select
          :value="font"
          title="Font"
          @mousedown.stop
          @change="setFont(($event.target as HTMLSelectElement).value)"
        >
          <option v-for="option in FONTS" :key="option.label" :value="option.value">
            {{ option.label }}
          </option>
        </select>
        <select
          :value="size"
          title="Size"
          @mousedown.stop
          @change="setSize(($event.target as HTMLSelectElement).value)"
        >
          <option v-for="option in SIZES" :key="option" :value="option">
            {{ option ? option.replace('px', '') : 'Size' }}
          </option>
        </select>
        <span class="notes-sep" />
        <button
          type="button"
          :class="{ on: active('bold') }"
          :title="`Bold (${mod}B)`"
          @click="chain().toggleBold().run()"
        >
          <b>B</b>
        </button>
        <button
          type="button"
          :class="{ on: active('italic') }"
          :title="`Italic (${mod}I)`"
          @click="chain().toggleItalic().run()"
        >
          <i>I</i>
        </button>
        <button
          type="button"
          :class="{ on: active('underline') }"
          :title="`Underline (${mod}U)`"
          @click="chain().toggleUnderline().run()"
        >
          <u>U</u>
        </button>
        <button
          type="button"
          :class="{ on: active('strike') }"
          title="Strikethrough"
          @click="chain().toggleStrike().run()"
        >
          <s>S</s>
        </button>
        <span class="notes-sep" />
        <button
          v-for="color in HIGHLIGHTS"
          :key="color"
          type="button"
          class="notes-swatch"
          :style="{ background: color }"
          :title="`Highlight (${mod}⇧H)`"
          @click="highlight(color)"
        />
        <button type="button" title="Remove highlight" @click="highlight(null)">⌀</button>
        <span class="notes-sep" />
        <button
          type="button"
          :class="{ on: active('bulletList') }"
          title="Bulleted list"
          @click="chain().toggleBulletList().run()"
        >
          •
        </button>
        <button
          type="button"
          :class="{ on: active('orderedList') }"
          title="Numbered list"
          @click="chain().toggleOrderedList().run()"
        >
          1.
        </button>
        <button
          type="button"
          :class="{ on: active('taskList') }"
          title="Checklist"
          @click="chain().toggleTaskList().run()"
        >
          ☑
        </button>
        <button
          type="button"
          :class="{ on: active('blockquote') }"
          title="Quote"
          @click="chain().toggleBlockquote().run()"
        >
          ❝
        </button>
        <button
          type="button"
          :class="{ on: active('codeBlock') }"
          title="Code"
          @click="chain().toggleCodeBlock().run()"
        >
          {}
        </button>
      </div>
      <EditorContent :editor="editor" class="notes-editor" />
    </template>

    <footer class="notes-footer">
      <span>{{ status ?? `${words} words` }}</span>
      <span>{{ mod }}P notes · {{ mod }}N new · {{ mod }}-click opens links</span>
    </footer>
  </main>
</template>
