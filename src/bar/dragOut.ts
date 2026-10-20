import { startDrag } from '@crabnebula/tauri-plugin-drag';
import { dragFinished, dragPrepare } from '../api';

/** How far the pointer moves with the button down before a press becomes a drag. */
const THRESHOLD = 6;

/**
 * Drags a file result or a clipboard history item out of the bar into another app, as a
 * file. The bar stays up during the drag and hides once it's dropped.
 */
export async function dragOut(source: { path?: string; clip?: number }): Promise<void> {
  const prepared = await dragPrepare(source);
  try {
    await startDrag({ item: prepared.files, icon: prepared.icon }, (payload) => {
      void dragFinished(payload.result === 'Dropped');
    });
  } catch (error) {
    await dragFinished(false);
    throw error;
  }
}

/**
 * Watches presses on a list's rows: once the pointer moves a few pixels with the button down,
 * `start` is called with the pressed row's index (once per press).
 */
export function dragGesture(start: (index: number) => void) {
  let pressed: { index: number; x: number; y: number } | null = null;
  return {
    down(event: MouseEvent, index: number) {
      if (event.button === 0) pressed = { index, x: event.screenX, y: event.screenY };
    },
    move(event: MouseEvent) {
      if (!pressed || !(event.buttons & 1)) {
        pressed = null;
        return;
      }
      const distance = Math.hypot(event.screenX - pressed.x, event.screenY - pressed.y);
      if (distance < THRESHOLD) return;
      const index = pressed.index;
      pressed = null;
      start(index);
    },
    up() {
      pressed = null;
    },
  };
}
