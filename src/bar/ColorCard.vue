<script setup lang="ts">
  import KeyHints from '../components/KeyHints.vue';
  import type { ColorCardModel } from './useColorCard';

  // The color card over the results: a swatch, HEX, RGB, HSL and OKLCH with the chosen
  // format highlighted, seven shades and the contrast with white and black. Up and down
  // pick a format, Enter copies it, and Tab steps the shades. The chosen shade is the
  // color shown.
  defineProps<{ color: ColorCardModel }>();
  defineEmits<{ format: [index: number]; shade: [index: number] }>();
</script>

<template>
  <section class="color-card" aria-label="Color" @mousedown.prevent>
    <span class="color-swatch" aria-hidden="true"
      ><span :style="{ background: color.swatch }"
    /></span>
    <ul class="color-formats">
      <li v-for="(format, index) in color.formats" :key="format.label">
        <button
          type="button"
          :class="{ on: index === color.format }"
          @click="$emit('format', index)"
        >
          <code>{{ format.text }}</code>
          <span>{{ format.label }}</span>
        </button>
      </li>
    </ul>
    <div class="color-shades" role="tablist" aria-label="Shades">
      <button
        v-for="(shade, index) in color.shades"
        :key="index"
        type="button"
        class="color-shade"
        role="tab"
        :aria-selected="index === color.shade"
        :class="{ on: index === color.shade }"
        :style="{ background: shade }"
        @click="$emit('shade', index)"
      />
    </div>
    <div class="color-foot">
      <span>{{ color.contrast }}</span>
      <KeyHints
        :hints="[
          { keys: 'up', label: 'Format' },
          { keys: 'tab', label: 'Shades' },
          { keys: 'enter', label: 'Copy' },
        ]"
      />
    </div>
  </section>
</template>
