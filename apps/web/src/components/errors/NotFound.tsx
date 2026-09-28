import { Link } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';
import { usePageTitle } from '../layout/use-page-title';

export function NotFound() {
  const { t } = useTranslation();
  usePageTitle(t('errors.notFound.title'));
  return (
    <section aria-labelledby="not-found-title" className="max-w-xl">
      <h1 id="not-found-title" className="text-2xl font-semibold">
        {t('errors.notFound.title')}
      </h1>
      <p className="mt-3 text-muted">{t('errors.notFound.body')}</p>
      <Link to="/" className="button-primary mt-6">
        {t('errors.notFound.home')}
      </Link>
    </section>
  );
}
