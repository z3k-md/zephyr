import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';
import tailwindcss from '@tailwindcss/vite';

declare const process: { env: { TAURI_DEV_HOST?: string }; versions: { bun?: string } };

const host = process.env.TAURI_DEV_HOST;

// https://vitejs.dev/config/
export default defineConfig(async () => ({
  plugins: [vue(), tailwindcss()],

  // `bun run vite` runs Vite under Node when Node is on PATH and under Bun when it isn't (Git
  // Bash). The two resolve Vite's own files to different paths, which changes the dep cache's
  // config hash, so sharing one cache rebuilt it on every switch and held the first page load.
  cacheDir: `node_modules/.vite/${process.versions.bun ? 'bun' : 'node'}`,

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: 'ws',
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell vite to ignore watching `src-tauri`
      ignored: ['**/src-tauri/**'],
    },
  },
}));
