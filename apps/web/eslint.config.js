// @ts-check
import pluginQuery from '@tanstack/eslint-plugin-query';
import pluginRouter from '@tanstack/eslint-plugin-router';
import { react } from '@worklume/eslint-config/react';
import { defineConfig } from 'eslint/config';

export default defineConfig(
  react({
    tsconfigRootDir: import.meta.dirname,
    ignores: ['src/routeTree.gen.ts', 'playwright-report/**', 'test-results/**'],
  }),
  pluginQuery.configs['flat/recommended'],
  pluginRouter.configs['flat/recommended'],
  {
    files: ['src/**/*.{ts,tsx}'],
    rules: {
      'no-restricted-imports': [
        'error',
        {
          paths: [{ name: 'zod', message: "Import from 'zod/mini' to keep the bundle small." }],
          patterns: [{ group: ['node:*'], message: 'Browser code cannot use Node.js built-ins.' }],
        },
      ],
    },
  },
  {
    // Build scripts report to the terminal.
    files: ['scripts/**'],
    rules: { 'no-console': 'off' },
  },
);
