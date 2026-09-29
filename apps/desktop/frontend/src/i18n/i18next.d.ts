import 'i18next';
import type { defaultNS, resources } from './index';

// Makes t('some.key') a type error when the key is missing from the English source.
declare module 'i18next' {
  interface CustomTypeOptions {
    defaultNS: typeof defaultNS;
    resources: (typeof resources)['en'];
    strictKeyChecks: true;
  }
}
