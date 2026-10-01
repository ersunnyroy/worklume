import i18next, { type i18n } from 'i18next';
import { initReactI18next } from 'react-i18next';
import en from './locales/en.json';

export const defaultNS = 'common';

/** Every shipped language. Add a locale file and an entry here to ship another. */
export const resources = {
  en: { common: en },
} as const;

export type Language = keyof typeof resources;

const fallbackLanguage: Language = 'en';

export function isSupportedLanguage(value: string): value is Language {
  return Object.hasOwn(resources, value);
}

/** Picks the first supported language from a preference list such as `navigator.languages`. */
export function negotiateLanguage(preferences: readonly string[]): Language {
  for (const preference of preferences) {
    const candidates = [preference, preference.split('-')[0] ?? preference];
    const match = candidates.find(isSupportedLanguage);
    if (match !== undefined) {
      return match;
    }
  }
  return fallbackLanguage;
}

export async function initI18n(language: Language): Promise<i18n> {
  await i18next.use(initReactI18next).init({
    lng: language,
    fallbackLng: fallbackLanguage,
    supportedLngs: Object.keys(resources),
    defaultNS,
    ns: [defaultNS],
    resources,
    // React escapes output already; escaping twice would show entities to users.
    interpolation: { escapeValue: false },
    react: { useSuspense: false },
  });
  document.documentElement.lang = i18next.resolvedLanguage ?? fallbackLanguage;
  document.documentElement.dir = i18next.dir();
  return i18next;
}
