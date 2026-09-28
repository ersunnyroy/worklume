// @ts-check
import assert from 'node:assert/strict';
import { join } from 'node:path';
import { describe, it } from 'node:test';
import { ESLint } from 'eslint';
import { base } from '../base.js';
import { react } from '../react.js';

const fixtures = join(import.meta.dirname, 'fixtures');

/**
 * Lint one fixture and return the rule id of every report.
 *
 * @param {import('eslint').Linter.Config[]} config
 * @param {string} file
 */
async function reportedRules(config, file) {
  const eslint = new ESLint({ cwd: fixtures, overrideConfigFile: true, overrideConfig: config });
  const [result] = await eslint.lintFiles([file]);
  assert.ok(result, `ESLint returned no result for ${file}`);
  return result.messages.map((message) => message.ruleId ?? `fatal: ${message.message}`);
}

describe('base config', () => {
  const config = base({ tsconfigRootDir: fixtures });

  it('accepts idiomatic code', async () => {
    assert.deepEqual(await reportedRules(config, 'clean.ts'), []);
  });

  it('reports unhandled promises', async () => {
    assert.deepEqual(await reportedRules(config, 'floating-promise.ts'), [
      '@typescript-eslint/no-floating-promises',
    ]);
  });

  it('reports switches that miss a union member', async () => {
    assert.deepEqual(await reportedRules(config, 'non-exhaustive-switch.ts'), [
      '@typescript-eslint/switch-exhaustiveness-check',
    ]);
  });
});

describe('react config', () => {
  const config = react({ tsconfigRootDir: fixtures });

  it('keeps the base rules', async () => {
    assert.deepEqual(await reportedRules(config, 'floating-promise.ts'), [
      '@typescript-eslint/no-floating-promises',
    ]);
  });

  it('reports hooks called conditionally', async () => {
    assert.deepEqual(await reportedRules(config, 'conditional-hook.ts'), [
      'react-hooks/rules-of-hooks',
    ]);
  });
});
