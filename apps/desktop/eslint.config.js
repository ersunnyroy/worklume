// @ts-check
import { react } from '@worklume/eslint-config/react';

export default react({
  tsconfigRootDir: import.meta.dirname,
  ignores: ['src-tauri/gen/**'],
});
