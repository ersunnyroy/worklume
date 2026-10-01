import react from '@vitejs/plugin-react';
import { defineConfig } from 'vitest/config';

// The webview UI. Tauri loads the dev server in `tauri dev` and embeds dist/ in `tauri build`.
export default defineConfig({
  root: 'frontend',
  plugins: [react()],
  clearScreen: false,
  build: {
    outDir: '../dist',
    emptyOutDir: true,
    // WebView2 on Windows is Chromium; macOS and Linux use WebKit (Ubuntu 22.04 ships 2.36+).
    target: ['es2021', 'chrome105', 'safari15'],
  },
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ['**/src-tauri/**'] },
  },
  test: {
    environment: 'jsdom',
    include: ['src/**/*.test.{ts,tsx}'],
    setupFiles: ['./src/test/setup.ts'],
    restoreMocks: true,
    // Times of day in tests read the same everywhere.
    env: { TZ: 'UTC' },
  },
});
