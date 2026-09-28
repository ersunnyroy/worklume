import { createFileRoute } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';
import { usePageTitle } from '../components/layout/use-page-title';

export const Route = createFileRoute('/')({
  component: HomePage,
});

function HomePage() {
  const { t } = useTranslation();
  usePageTitle(t('nav.home'));
  return (
    <section aria-labelledby="home-title" className="max-w-2xl">
      <h1 id="home-title" className="text-2xl font-semibold">
        {t('home.title')}
      </h1>
      <p className="mt-3 text-muted">{t('home.body')}</p>
    </section>
  );
}
