// @ts-check
import js from '@eslint/js';
import prettier from 'eslint-config-prettier/flat';
import { defineConfig, globalIgnores } from 'eslint/config';
import globals from 'globals';
import tseslint from 'typescript-eslint';

/**
 * @typedef {object} WorklumeLintOptions
 * @property {string} tsconfigRootDir Directory of the consuming package, normally
 *   `import.meta.dirname`, used to locate its tsconfig for type-aware rules.
 * @property {string[]} [ignores] Extra glob patterns to skip in this package.
 */

/** Build output and dependency folders that are never linted. */
const GENERATED = [
  '**/node_modules/**',
  '**/dist/**',
  '**/build/**',
  '**/coverage/**',
  '**/.turbo/**',
  '**/target/**',
  '**/vendor/**',
];

/**
 * Type-aware ESLint config shared by every TypeScript package.
 *
 * @param {WorklumeLintOptions} options
 */
export function base(options) {
  return defineConfig(
    globalIgnores([...GENERATED, ...(options.ignores ?? [])]),
    js.configs.recommended,
    tseslint.configs.strictTypeChecked,
    tseslint.configs.stylisticTypeChecked,
    {
      languageOptions: {
        parserOptions: {
          projectService: true,
          tsconfigRootDir: options.tsconfigRootDir,
        },
      },
      linterOptions: {
        reportUnusedDisableDirectives: 'error',
      },
      rules: {
        eqeqeq: ['error', 'always'],
        'no-console': ['error', { allow: ['warn', 'error'] }],
        '@typescript-eslint/consistent-type-imports': [
          'error',
          { fixStyle: 'inline-type-imports' },
        ],
        '@typescript-eslint/no-import-type-side-effects': 'error',
        '@typescript-eslint/no-unused-vars': [
          'error',
          { argsIgnorePattern: '^_', varsIgnorePattern: '^_', caughtErrors: 'all' },
        ],
        '@typescript-eslint/restrict-template-expressions': ['error', { allowNumber: true }],
        '@typescript-eslint/switch-exhaustiveness-check': [
          'error',
          { considerDefaultExhaustiveForUnions: true },
        ],
      },
    },
    {
      // Plain JavaScript (config files, scripts) has no type information.
      files: ['**/*.{js,mjs,cjs}'],
      extends: [tseslint.configs.disableTypeChecked],
      languageOptions: { globals: globals.node },
    },
    // Formatting belongs to Prettier; this must stay last.
    prettier,
  );
}
