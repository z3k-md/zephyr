import { afterEach, expect, test, vi } from 'vitest';
import { build } from 'vite';
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';

afterEach(() => vi.unstubAllGlobals());

test('production window loaders preload the selected window stylesheet', async () => {
  // Build the real routing script with tiny stand-in views. Dev-mode imports do not
  // exercise Vite's production CSS preloader, where this regression occurred.
  const app = await readFile(resolve('src/App.vue'), 'utf8');
  const script = app.match(/<script setup lang="ts">([\s\S]*?)<\/script>/)[1];
  const result = await build({
    configFile: false,
    logLevel: 'silent',
    plugins: [
      {
        name: 'window-style-fixture',
        enforce: 'pre',
        resolveId(id) {
          if (id === 'window-entry.ts' || id === 'vue' || id.endsWith('.vue')) return `\0${id}`;
          if (id.endsWith('.css')) return id;
        },
        load(id) {
          if (id === '\0window-entry.ts') return `${script}\nexport default Page;`;
          if (id === '\0vue') return 'export const defineAsyncComponent = loader => loader;';
          if (id.endsWith('.vue')) {
            const name = id.split('/').at(-1).replace('.vue', '');
            return `import '${name}.css'; export default '${name}';`;
          }
          if (id.endsWith('.css')) return `.${id.replace('.css', '')} { display: flex; }`;
        },
      },
    ],
    build: {
      write: false,
      minify: true,
      rollupOptions: { input: 'window-entry.ts', preserveEntrySignatures: 'strict' },
    },
  });
  if (Array.isArray(result) || !('output' in result)) throw new Error('Expected one build');
  const entry = result.output.find((file) => file.type === 'chunk' && file.isEntry);
  if (!entry || entry.type !== 'chunk') throw new Error('Missing entry');

  // Run the emitted preloader, recording stylesheet requests and checking the files
  // exist. Only the final JS import is replaced; it isn't needed to test CSS loading.
  const code = entry.code
    .replace(/\bimport\.meta/g, 'moduleMeta')
    .replace(/\bimport\(/g, 'loadModule(')
    .replace(/export\{(\w+) as default\};?/, 'return $1;');
  for (const [search, hash, view] of [
    ['', '', 'BarView'],
    ['?view=osd', '', 'OsdView'],
    ['?view=settings', '', 'SettingsView'],
    ['', '#settings', 'SettingsView'],
    ['?view=grid', '', 'GridView'],
    ['?view=update', '', 'UpdateView'],
    ['?view=notes', '', 'NotesView'],
    ['?view=onboarding', '', 'OnboardingView'],
    ['?view=unknown', '', 'BarView'],
  ]) {
    const styles = [];
    vi.stubGlobal('window', { location: { search, hash } });
    vi.stubGlobal('document', {
      getElementsByTagName: () => [],
      querySelector: () => null,
      createElement: () => ({
        addEventListener(event, callback) {
          if (event === 'load') queueMicrotask(callback);
        },
      }),
      head: {
        appendChild(link) {
          if (link.rel !== 'stylesheet') return;
          const path = new URL(link.href, 'https://tauri.localhost').pathname.replace(/^\//, '');
          expect(result.output.some((file) => file.fileName === path)).toBe(true);
          styles.push(path);
        },
      },
    });
    const loader = new Function('loadModule', 'moduleMeta', code)(() => Promise.resolve({}), {
      url: 'https://tauri.localhost/assets/entry.js',
    });
    await loader();
    expect(styles, `${search}${hash} must load its own CSS`).toEqual([
      expect.stringMatching(new RegExp(`${view}-.*\\.css$`)),
    ]);
  }
});
