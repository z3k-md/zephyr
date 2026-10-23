import { onMounted, onUnmounted } from 'vue';

/** Listens for an event on the window while the component is mounted. */
export function useWindowListener<K extends keyof WindowEventMap>(
  type: K,
  listener: (event: WindowEventMap[K]) => void,
  capture = false
) {
  onMounted(() => window.addEventListener(type, listener, capture));
  onUnmounted(() => window.removeEventListener(type, listener, capture));
}
