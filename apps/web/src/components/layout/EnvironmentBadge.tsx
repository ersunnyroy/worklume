import { useTranslation } from 'react-i18next';
import type { Environment } from '../../config/runtime-config';

/** Marks non-production deployments so nobody mistakes test data for real data. */
export function EnvironmentBadge({ environment }: { readonly environment: Environment }) {
  const { t } = useTranslation();
  if (environment === 'production') {
    return null;
  }
  return (
    <span className="rounded-full bg-warning px-2.5 py-0.5 text-xs font-medium text-warning-ink">
      {t('shell.environmentBadge', { environment: t(`environments.${environment}`) })}
    </span>
  );
}
