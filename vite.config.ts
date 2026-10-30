import { defineConfig, type Plugin } from 'vite';
import vue from '@vitejs/plugin-vue';
import { execSync } from 'node:child_process';
import { appendFileSync, mkdirSync, writeFileSync } from 'node:fs';

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

// Dev builds write each window's console warnings, errors and uncaught exceptions to
// logs/frontend.log, so a broken page can be read without opening devtools.
const CONSOLE_LOG = 'logs/frontend.log';
const CONSOLE_MODULE = '/@zephyr-console';
const CONSOLE_CLIENT = `
const send = (level, parts) => {
  const text = parts
    .map((part) =>
      part instanceof Error
        ? part.stack || String(part)
        : typeof part === 'string'
          ? part
          : (() => { try { return JSON.stringify(part); } catch { return String(part); } })()
    )
    .join(' ');
  import.meta.hot?.send('zephyr:console', { level, page: location.pathname + location.search, text });
};
for (const level of ['warn', 'error']) {
  const original = console[level].bind(console);
  console[level] = (...parts) => { original(...parts); send(level, parts); };
}
addEventListener('error', (event) => send('uncaught', [event.error ?? event.message]));
addEventListener('unhandledrejection', (event) => send('unhandled', [event.reason]));
`;

function consoleToFile(): Plugin {
  return {
    name: 'zephyr-console-to-file',
    apply: 'serve',
    resolveId: (id) => (id === CONSOLE_MODULE ? id : null),
    load: (id) => (id === CONSOLE_MODULE ? CONSOLE_CLIENT : null),
    transformIndexHtml: () => [
      { tag: 'script', attrs: { type: 'module', src: CONSOLE_MODULE }, injectTo: 'head-prepend' },
    ],
    configureServer(server) {
      mkdirSync('logs', { recursive: true });
      writeFileSync(CONSOLE_LOG, '');
      server.ws.on('zephyr:console', (data: { level: string; page: string; text: string }) => {
        const time = new Date().toTimeString().slice(0, 8);
        appendFileSync(CONSOLE_LOG, `${time} ${data.level} ${data.page}\n${data.text}\n\n`);
      });
    },
  };
}

// https://vitejs.dev/config/
export default defineConfig(async () => ({
  plugins: [vue(), consoleToFile()],
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
