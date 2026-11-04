<script setup lang="ts">
  import './picker.css';
  import { onMounted, onUnmounted, ref } from 'vue';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { pickerCancel, pickerChoose } from '../api';
  import { isMac } from '../keys';

  // The screen color picker: the frozen screenshot, and a loupe of the pixel under the
  // cursor. Arrow keys nudge one pixel (Shift, ten). Click or Enter copies that pixel's
  // hex; Esc or a right-click cancels.

  /** Pixels on a side of the loupe. Odd, so one pixel sits in the middle. */
  const N = 11;
  const HALF = (N - 1) / 2;
  /** How many screen pixels one loupe cell covers. */
  const CELL = 12;

  const screenEl = ref<HTMLCanvasElement | null>(null);
  const loupeEl = ref<HTMLCanvasElement | null>(null);
  const hex = ref('');
  const loupeStyle = ref({ left: '0px', top: '0px' });

  let width = 0;
  let height = 0;
  let pixels = new Uint8ClampedArray();
  let x = 0;
  let y = 0;
  /** Arrow keys own the pixel until the pointer moves again. */
  let nudged = false;
  let unlisten: UnlistenFn | null = null;
  /** The frame is served once per capture, so a second fetch while this one runs gets nothing. */
  let loading = false;

  function frameUrl(): string {
    const base = isMac ? 'picker://localhost/frame' : 'http://picker.localhost/frame';
    return `${base}?t=${Date.now()}`;
  }

  async function load() {
    if (loading) return;
    loading = true;
    try {
      const response = await fetch(frameUrl());
      if (!response.ok) return;
      const nextWidth = Number(response.headers.get('X-Width'));
      const nextHeight = Number(response.headers.get('X-Height'));
      const bytes = new Uint8Array(await response.arrayBuffer());
      if (!nextWidth || !nextHeight || bytes.length < nextWidth * nextHeight * 4) return;
      width = nextWidth;
      height = nextHeight;
      pixels = new Uint8ClampedArray(bytes.buffer, bytes.byteOffset, nextWidth * nextHeight * 4);
      const canvas = screenEl.value;
      const context = canvas?.getContext('2d');
      if (!canvas || !context) return;
      canvas.width = width;
      canvas.height = height;
      context.putImageData(new ImageData(pixels, width, height), 0, 0);
      place(Math.floor(width / 2), Math.floor(height / 2));
    } catch {
      // The picker may have closed, or this capture's frame was already taken.
    } finally {
      loading = false;
    }
  }

  /** The window is reused, so the old frame goes before it hides: the next pick starts
   *  blank and takes no click until its own frame loads. */
  function clear() {
    width = 0;
    height = 0;
    pixels = new Uint8ClampedArray();
    hex.value = '';
    if (screenEl.value) screenEl.value.width = 0;
  }

  async function choose() {
    const picked = hex.value;
    if (!picked) return;
    clear();
    try {
      await pickerChoose(picked);
    } catch {
      // A failed copy closes the picker and reports on the bar.
    }
  }

  function cancel() {
    clear();
    void pickerCancel();
  }

  function sample(px: number, py: number): [number, number, number] {
    const sx = Math.min(width - 1, Math.max(0, px));
    const sy = Math.min(height - 1, Math.max(0, py));
    const at = (sy * width + sx) * 4;
    return [pixels[at] ?? 0, pixels[at + 1] ?? 0, pixels[at + 2] ?? 0];
  }

  function place(px: number, py: number) {
    if (!width || !height) return;
    x = Math.min(width - 1, Math.max(0, px));
    y = Math.min(height - 1, Math.max(0, py));
    const [r, g, b] = sample(x, y);
    hex.value = `#${[r, g, b].map((channel) => channel.toString(16).padStart(2, '0')).join('')}`;
    drawLoupe();
    moveLoupe();
  }

  function drawLoupe() {
    const canvas = loupeEl.value;
    const context = canvas?.getContext('2d');
    if (!canvas || !context) return;
    canvas.width = N * CELL;
    canvas.height = N * CELL;
    for (let row = 0; row < N; row++) {
      for (let col = 0; col < N; col++) {
        const [r, g, b] = sample(x + col - HALF, y + row - HALF);
        context.fillStyle = `rgb(${r} ${g} ${b})`;
        context.fillRect(col * CELL, row * CELL, CELL, CELL);
      }
    }
    context.strokeStyle = 'rgba(0, 0, 0, 0.45)';
    context.lineWidth = 1;
    for (let i = 1; i < N; i++) {
      context.beginPath();
      context.moveTo(i * CELL + 0.5, 0);
      context.lineTo(i * CELL + 0.5, N * CELL);
      context.moveTo(0, i * CELL + 0.5);
      context.lineTo(N * CELL, i * CELL + 0.5);
      context.stroke();
    }
    context.strokeStyle = '#000';
    context.lineWidth = 3;
    context.strokeRect(HALF * CELL + 1.5, HALF * CELL + 1.5, CELL - 3, CELL - 3);
    context.strokeStyle = '#fff';
    context.lineWidth = 1.5;
    context.strokeRect(HALF * CELL + 1.5, HALF * CELL + 1.5, CELL - 3, CELL - 3);
  }

  function moveLoupe() {
    const size = N * CELL + 22;
    const gap = 16;
    let left = (x / width) * window.innerWidth + gap;
    let top = (y / height) * window.innerHeight + gap;
    if (left + size > window.innerWidth) left = (x / width) * window.innerWidth - size - gap;
    if (top + size > window.innerHeight) top = (y / height) * window.innerHeight - size - gap;
    loupeStyle.value = { left: `${Math.max(4, left)}px`, top: `${Math.max(4, top)}px` };
  }

  function fromPointer(event: MouseEvent) {
    if (!width || !height || !window.innerWidth || !window.innerHeight) return;
    nudged = false;
    place(
      Math.floor((event.clientX / window.innerWidth) * width),
      Math.floor((event.clientY / window.innerHeight) * height)
    );
  }

  function onKey(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault();
      cancel();
      return;
    }
    if (event.key === 'Enter') {
      event.preventDefault();
      if (!event.repeat) choose();
      return;
    }
    const step = event.shiftKey ? 10 : 1;
    const move: Record<string, [number, number]> = {
      ArrowLeft: [-step, 0],
      ArrowRight: [step, 0],
      ArrowUp: [0, -step],
      ArrowDown: [0, step],
    };
    const delta = move[event.key];
    if (!delta) return;
    event.preventDefault();
    nudged = true;
    place(x + delta[0], y + delta[1]);
  }

  function onPointerMove(event: MouseEvent) {
    if (nudged) {
      nudged = false;
      return;
    }
    fromPointer(event);
  }

  function onPointerDown(event: MouseEvent) {
    if (event.button === 2) {
      event.preventDefault();
      cancel();
      return;
    }
    if (event.button !== 0) return;
    fromPointer(event);
    choose();
  }

  function onContext(event: Event) {
    event.preventDefault();
    cancel();
  }

  onMounted(async () => {
    document.documentElement.classList.add('picker-page');
    window.addEventListener('keydown', onKey);
    window.addEventListener('mousemove', onPointerMove);
    window.addEventListener('mousedown', onPointerDown);
    window.addEventListener('contextmenu', onContext);
    unlisten = await listen('picker-shown', () => void load());
    void load();
  });

  onUnmounted(() => {
    document.documentElement.classList.remove('picker-page');
    window.removeEventListener('keydown', onKey);
    window.removeEventListener('mousemove', onPointerMove);
    window.removeEventListener('mousedown', onPointerDown);
    window.removeEventListener('contextmenu', onContext);
    unlisten?.();
  });
</script>

<template>
  <canvas ref="screenEl" class="picker-screen" />
  <div class="picker-loupe" :style="loupeStyle">
    <canvas ref="loupeEl" />
    <span class="picker-hex">{{ hex }}</span>
  </div>
</template>
