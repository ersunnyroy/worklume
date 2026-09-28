// @ts-check
import { base } from './base.js';

export default base({
  tsconfigRootDir: import.meta.dirname,
  // Fixtures break the rules on purpose so the tests can check they fire.
  ignores: ['test/fixtures/**'],
});
