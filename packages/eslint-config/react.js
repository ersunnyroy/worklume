// @ts-check
import { defineConfig } from 'eslint/config';
import reactHooks from 'eslint-plugin-react-hooks';
import globals from 'globals';
import { base } from './base.js';

/**
 * ESLint config for React code: the web app, the mobile app and the desktop agent's UI.
 *
 * @param {import('./base.js').WorklumeLintOptions} options
 */
export function react(options) {
  return defineConfig(base(options), {
    files: ['**/*.{ts,tsx}'],
    languageOptions: { globals: globals.browser },
    extends: [reactHooks.configs.flat['recommended-latest']],
  });
}
