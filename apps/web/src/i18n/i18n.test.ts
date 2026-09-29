import i18next from 'i18next';
import { describe, expect, it } from 'vitest';
import { negotiateLanguage, resources } from '.';

function leafValues(tree: object): unknown[] {
  return Object.values(tree).flatMap((value: unknown) =>
    typeof value === 'object' && value !== null ? leafValues(value) : [value],
  );
}

describe('i18n', () => {
  it('sets the document language and direction', () => {
    expect(document.documentElement.lang).toBe('en');
    expect(document.documentElement.dir).toBe('ltr');
  });

  it('translates keys from the English source', () => {
    expect(i18next.t('app.name')).toBe('Worklume');
  });

  it('has a non-empty string for every English key', () => {
    for (const value of leafValues(resources.en.common)) {
      expect(typeof value === 'string' && value.trim() !== '').toBe(true);
    }
  });

  it.each([
    [['en-IN', 'hi'], 'en'],
    [['fr-FR', 'en-US'], 'en'],
    [['de'], 'en'],
    [[], 'en'],
  ])('negotiates %j to %s', (preferences, expected) => {
    expect(negotiateLanguage(preferences)).toBe(expected);
  });
});
