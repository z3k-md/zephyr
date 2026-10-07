<script setup lang="ts">
  import { nextTick, onMounted, onUnmounted, ref } from 'vue';

  // Opens a glass menu in place of the native popup for every <select> inside `root`. The
  // select stays the source of truth: picking sets its value and fires `change`, so v-model
  // and @change handlers work unchanged.
  const props = defineProps<{ root: HTMLElement | null }>();

  interface Option {
    value: string;
    label: string;
    disabled: boolean;
  }

  const open = ref(false);
  const options = ref<Option[]>([]);
  const active = ref(0);
  const current = ref('');
  const style = ref<Record<string, string>>({});
  const listEl = ref<HTMLElement | null>(null);
  let target: HTMLSelectElement | null = null;
  let typed = '';
  let typedAt = 0;

  function selectFrom(event: Event): HTMLSelectElement | null {
    const element = event.target as HTMLElement | null;
    if (!(element instanceof HTMLSelectElement)) return null;
    if (!props.root?.contains(element) || element.disabled) return null;
    return element;
  }

  async function show(select: HTMLSelectElement) {
    target = select;
    options.value = Array.from(select.options).map((option) => ({
      value: option.value,
      label: option.label || option.text,
      disabled: option.disabled,
    }));
    current.value = select.value;
    active.value = Math.max(0, select.selectedIndex);
    const rect = select.getBoundingClientRect();
    const room = window.innerHeight - rect.bottom - 12;
    const height = Math.min(300, options.value.length * 34 + 10);
    const above = room < height && rect.top > room;
    style.value = {
      left: `${rect.left}px`,
      width: `${rect.width}px`,
      ...(above
        ? { bottom: `${window.innerHeight - rect.top + 4}px` }
        : { top: `${rect.bottom + 4}px` }),
      maxHeight: `${Math.max(120, above ? rect.top - 12 : room)}px`,
    };
    open.value = true;
    await nextTick();
    listEl.value
      ?.querySelector<HTMLElement>(`[data-index="${active.value}"]`)
      ?.scrollIntoView({ block: 'nearest' });
  }

  function close(refocus = true) {
    if (!open.value) return;
    open.value = false;
    if (refocus) target?.focus();
  }

  function pick(index: number) {
    const option = options.value[index];
    if (!option || option.disabled || !target) return;
    if (target.value !== option.value) {
      target.value = option.value;
      target.dispatchEvent(new Event('input', { bubbles: true }));
      target.dispatchEvent(new Event('change', { bubbles: true }));
    }
    close();
  }

  function step(delta: number) {
    const count = options.value.length;
    for (let tries = 0, index = active.value; tries < count; tries++) {
      index = (index + delta + count) % count;
      if (!options.value[index].disabled) {
        active.value = index;
        break;
      }
    }
    void nextTick(() =>
      listEl.value
        ?.querySelector<HTMLElement>(`[data-index="${active.value}"]`)
        ?.scrollIntoView({ block: 'nearest' })
    );
  }

  function onMouseDown(event: MouseEvent) {
    const select = selectFrom(event);
    if (select) {
      event.preventDefault();
      select.focus();
      if (open.value && target === select) close();
      else void show(select);
      return;
    }
    if (open.value && !listEl.value?.contains(event.target as Node)) close(false);
  }

  function onKeyDown(event: KeyboardEvent) {
    if (!open.value) {
      const select = selectFrom(event);
      if (select && [' ', 'Enter', 'ArrowDown', 'ArrowUp'].includes(event.key)) {
        event.preventDefault();
        void show(select);
      }
      return;
    }
    if (event.key === 'Escape' || event.key === 'Tab') {
      if (event.key === 'Escape') event.preventDefault();
      close(event.key === 'Escape');
    } else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      step(event.key === 'ArrowDown' ? 1 : -1);
    } else if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      pick(active.value);
    } else if (event.key.length === 1 && !event.metaKey && !event.ctrlKey) {
      // Type-ahead: jump to the first option starting with what was typed.
      event.preventDefault();
      typed = Date.now() - typedAt > 700 ? event.key : typed + event.key;
      typedAt = Date.now();
      const index = options.value.findIndex(
        (option) => !option.disabled && option.label.toLowerCase().startsWith(typed.toLowerCase())
      );
      if (index >= 0) active.value = index;
    } else {
      return;
    }
    event.stopPropagation();
  }

  function onScroll(event: Event) {
    if (open.value && !listEl.value?.contains(event.target as Node)) close(false);
  }

  function onBlur() {
    close(false);
  }

  onMounted(() => {
    window.addEventListener('mousedown', onMouseDown, true);
    window.addEventListener('keydown', onKeyDown, true);
    window.addEventListener('scroll', onScroll, true);
    window.addEventListener('blur', onBlur);
  });

  onUnmounted(() => {
    window.removeEventListener('mousedown', onMouseDown, true);
    window.removeEventListener('keydown', onKeyDown, true);
    window.removeEventListener('scroll', onScroll, true);
    window.removeEventListener('blur', onBlur);
  });
</script>

<template>
  <ul v-if="open" ref="listEl" class="select-menu" role="listbox" :style="style">
    <li
      v-for="(option, index) in options"
      :key="option.value"
      :data-index="index"
      role="option"
      :aria-selected="option.value === current"
      :aria-disabled="option.disabled || undefined"
      class="select-option"
      :class="{ active: index === active, disabled: option.disabled }"
      @mousemove="!option.disabled && (active = index)"
      @mousedown.prevent
      @click="pick(index)"
    >
      <svg
        class="select-tick"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2.2"
        stroke-linecap="round"
        stroke-linejoin="round"
        aria-hidden="true"
      >
        <path v-if="option.value === current" d="m5 12.5 4.5 4.5L19 7.5" />
      </svg>
      <span>{{ option.label }}</span>
    </li>
  </ul>
</template>
