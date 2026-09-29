import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';

/** Sets the browser tab title for the current page (WCAG 2.4.2 Page Titled). */
export function usePageTitle(page: string): void {
  const { t } = useTranslation();
  const title = t('shell.pageTitle', { page });
  useEffect(() => {
    document.title = title;
  }, [title]);
}
