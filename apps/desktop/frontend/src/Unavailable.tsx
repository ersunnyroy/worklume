import { useTranslation } from 'react-i18next';
import type { UnavailableReason } from './ipc/bindings.gen';

/** Why the agent can't track time here, and what happens next. */
export function Unavailable({ reason }: { readonly reason: UnavailableReason }) {
  const { t } = useTranslation();
  return (
    <section className="unavailable" role="alert" aria-labelledby="unavailable-heading">
      <h2 id="unavailable-heading">{t('agent.unavailable.heading')}</h2>
      <p>{t(`agent.unavailable.${reason}`)}</p>
    </section>
  );
}
