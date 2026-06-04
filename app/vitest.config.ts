import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  plugins: [svelte({ hot: false })],
  resolve: {
    alias: {
      $lib: fileURLToPath(new URL('./src/lib', import.meta.url)),
    },
    // Use Svelte's client (browser) entry under jsdom so component tests can
    // `mount()` — otherwise the plugin resolves the server build.
    conditions: ['browser'],
  },
  test: {
    environment: 'jsdom',
    include: ['src/**/*.{test,spec}.{ts,js}'],
    globals: true,
    setupFiles: ['./vitest.setup.ts'],
  },
});
