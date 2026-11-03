// The color card: a hex, a CSS color function, or "color red" / "red color". Up and down
// pick a format, Enter copies it, and Tab steps through the shades. The chosen shade
// becomes the color the card shows.

import { computed, ref, watch, type Ref } from 'vue';
import { hideBar } from '../api';
import { rememberAnswer } from './answerHistory';
import { colorFor, contrastAgainst, formatsFor, type Rgba } from './color';

export const FORMAT_ORDER = ['hex', 'rgb', 'hsl', 'oklch'] as const;

export interface ColorCardModel {
  swatch: string;
  formats: { label: string; text: string }[];
  format: number;
  shades: string[];
  shade: number;
  contrast: string;
  /** The format Enter copies. */
  copy: string;
}

export function useColorCard(ctx: {
  query: Ref<string>;
  /** The card shows on the main bar with no scope, and gives way to the ⌘K menu. */
  shown: () => boolean;
  menuOpen: Ref<boolean>;
  notice: Ref<string | null>;
}) {
  const { query, menuOpen, notice } = ctx;
  const parsed = computed(() => (ctx.shown() && !menuOpen.value ? colorFor(query.value) : null));
  const formatIndex = ref(0);
  const shadeIndex = ref(0);

  watch(
    () => (parsed.value ? query.value.trim().toLowerCase() : ''),
    () => {
      shadeIndex.value = parsed.value?.shadeIndex ?? 0;
    }
  );

  const card = computed((): ColorCardModel | null => {
    const color = parsed.value;
    if (!color) return null;
    const at =
      ((shadeIndex.value % color.shades.length) + color.shades.length) % color.shades.length;
    const rgba = color.shades[at] ?? color.rgba;
    const formats = formatsFor(rgba);
    const format =
      ((formatIndex.value % FORMAT_ORDER.length) + FORMAT_ORDER.length) % FORMAT_ORDER.length;
    return {
      swatch: formats.hex,
      formats: FORMAT_ORDER.map((id) => ({ label: id.toUpperCase(), text: formats[id] })),
      format,
      shades: color.shades.map((shade) => formatsFor(shade).hex),
      shade: at,
      contrast: contrastLine(rgba),
      copy: formats[FORMAT_ORDER[format]],
    };
  });

  /** Up or down on the card: the next format (up is the one before). */
  function stepFormat(by: 1 | -1) {
    formatIndex.value = (formatIndex.value + by + FORMAT_ORDER.length) % FORMAT_ORDER.length;
  }

  /** Tab on the card: the next shade (Shift+Tab, the one before). */
  function stepShade(by: 1 | -1) {
    const count = parsed.value?.shades.length ?? 0;
    if (!count) return;
    shadeIndex.value = (shadeIndex.value + by + count) % count;
  }

  function pickFormat(index: number) {
    formatIndex.value = index;
  }

  function pickShade(index: number) {
    shadeIndex.value = index;
  }

  /** Enter on the card: copy the chosen format, and remember it with the other answers. */
  async function copyFormat() {
    const text = card.value?.copy;
    if (!text) return;
    rememberAnswer(query.value, text);
    try {
      await navigator.clipboard.writeText(text);
      query.value = '';
      await hideBar();
    } catch {
      notice.value = "Couldn't copy it";
    }
  }

  return { card, stepFormat, stepShade, pickFormat, pickShade, copyFormat };
}

function contrastLine(rgba: Rgba): string {
  const white = ratio(contrastAgainst(rgba, 'white'));
  const black = ratio(contrastAgainst(rgba, 'black'));
  return `${white} on white · ${black} on black`;
}

function ratio(value: number): string {
  const rounded = Math.round(value * 100) / 100;
  return `${rounded}:1`;
}
