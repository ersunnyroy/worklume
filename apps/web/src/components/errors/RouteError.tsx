import { useRouter, type ErrorComponentProps } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';
import { usePageTitle } from '../layout/use-page-title';

/** A support reference carried by API errors (problem+json `request_id`), if the error has one. */
export function errorReference(error: unknown): string | null {
  if (typeof error === 'object' && error !== null && 'requestId' in error) {
    const { requestId } = error;
    return typeof requestId === 'string' && requestId !== '' ? requestId : null;
  }
  return null;
}

/**
 * Replaces a route's content when it fails to load or render. Error details stay out of the
 * UI in production builds; the reference lets support find the request in the logs.
 */
export function RouteError({ error, reset }: ErrorComponentProps) {
  const { t } = useTranslation();
  usePageTitle(t('errors.unexpected.title'));
  const router = useRouter();
  const reference = errorReference(error);

  function retry() {
    reset();
    void router.invalidate();
  }

  return (
    <section aria-labelledby="route-error-title" role="alert" className="max-w-xl">
      <h1 id="route-error-title" className="text-2xl font-semibold">
        {t('errors.unexpected.title')}
      </h1>
      <p className="mt-3 text-muted">{t('errors.unexpected.body')}</p>
      {reference !== null && (
        <p className="mt-2 text-sm text-muted">{t('errors.unexpected.reference', { reference })}</p>
      )}
      {import.meta.env.DEV && (
        <pre className="mt-4 overflow-auto rounded-md border border-line p-3 text-sm">
          {error instanceof Error ? error.message : String(error)}
        </pre>
      )}
      <button type="button" onClick={retry} className="button-primary mt-6">
        {t('errors.unexpected.retry')}
      </button>
    </section>
  );
}
