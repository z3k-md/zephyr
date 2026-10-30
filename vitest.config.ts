import { defineConfig } from 'vitest/config';

// Frontend unit tests and the production loader regression, under Node with no DOM.
export default defineConfig({
  test: {
    include: ['src/**/*.test.{ts,js}'],
    environment: 'node',
  },
});
