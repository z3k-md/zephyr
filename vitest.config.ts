import { defineConfig } from 'vitest/config';

// The frontend's unit tests: plain functions, run under Node with no DOM.
export default defineConfig({
  test: {
    include: ['src/**/*.test.ts'],
    environment: 'node',
  },
});
