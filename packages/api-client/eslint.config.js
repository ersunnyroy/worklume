// @ts-check
import { base } from '@worklume/eslint-config';

export default base({
  tsconfigRootDir: import.meta.dirname,
  ignores: ['src/schema.gen.ts'],
});
