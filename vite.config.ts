import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';
import { execSync } from 'node:child_process';

declare const process: { env: { TAURI_DEV_HOST?: string }; versions: { bun?: string } };

const host = process.env.TAURI_DEV_HOST;

// Dev builds name the commit they run (Settings › About), since the version in package.json
// stays put: releases take theirs from the tag. "9d31fae (after v2.10.0)".
function devBuild(): string {
  const git = (args: string) => {
    try {
      return execSync(`git ${args}`, {
        encoding: 'utf8',
        stdio: ['ignore', 'pipe', 'ignore'],
      }).trim();
    } catch {
      return '';
    }
  };
  const commit = git('rev-parse --short HEAD');
  const tag = git('describe --tags --abbrev=0 --match v*');
  return [commit, tag ? `(after ${tag})` : ''].filter(Boolean).join(' ');
}

// https://vitejs.dev/config/
export default defineConfig(async () => ({
  plugins: [vue()],
  define: {
    __DEV_BUILD__: JSON.stringify(devBuild()),
  },

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
