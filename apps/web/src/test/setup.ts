import '@testing-library/jest-dom/vitest';
import { cleanup } from '@testing-library/react';
import { afterEach, beforeAll } from 'vitest';
import { initI18n } from '../i18n';

beforeAll(async () => {
  await initI18n('en');
});

afterEach(() => {
  cleanup();
});

// jsdom has no layout, so scroll restoration's scrollTo is a no-op here.
window.scrollTo = () => undefined;
