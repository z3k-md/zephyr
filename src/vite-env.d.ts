/// <reference types="vite/client" />

declare module '*.vue' {
  import type { DefineComponent } from 'vue';
  const component: DefineComponent<{}, {}, any>;
  export default component;
}

/** Dev builds: the commit they run and the release before it ("9d31fae (after v2.10.0)"). */
declare const __DEV_BUILD__: string;
