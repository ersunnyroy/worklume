// @ts-check
import { react } from '@worklume/eslint-config/react';
import { defineConfig } from 'eslint/config';

export default defineConfig(
  react({
    tsconfigRootDir: import.meta.dirname,
    ignores: ['src-tauri/gen/**', 'frontend/src/ipc/bindings.gen.ts'],
  }),
  {
    // Scripts report to the terminal.
    files: ['scripts/**'],
    rules: { 'no-console': 'off' },
  },
);
