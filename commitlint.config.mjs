/**
 * Conventional Commits, with scopes matching the monorepo areas.
 * See CONTRIBUTING.md for examples.
 * @type {import('@commitlint/types').UserConfig}
 */
export default {
  extends: ['@commitlint/config-conventional'],
  rules: {
    'scope-enum': [
      2,
      'always',
      ['repo', 'api', 'web', 'desktop', 'mobile', 'ext', 'infra', 'docs', 'deps', 'ci', 'config'],
    ],
    'subject-case': [2, 'never', ['upper-case', 'pascal-case', 'start-case']],
    'header-max-length': [2, 'always', 100],
  },
};
