import { useTranslation } from 'react-i18next';
import type { CapabilitiesDto } from './ipc/bindings.gen';

interface DeviceLimitsProps {
  readonly capabilities: CapabilitiesDto | null;
  /** The member wants to be asked again about sharing their screen, after declining. */
  readonly onAskAgain: () => void;
}

/**
 * What Worklume can't fully measure on this desktop, and why, from the agent's capability
 * report. Shows nothing while everything is measured or before the first report. A member
 * who declined screen sharing can ask to be asked again.
 */
export function DeviceLimits({ capabilities, onAskAgain }: DeviceLimitsProps) {
  const { t } = useTranslation();
  const limited = (capabilities?.features ?? []).filter((feature) => feature.status !== 'granted');
  if (limited.length === 0) {
    return null;
  }
  return (
    <section className="limits" aria-labelledby="limits-heading">
      <h2 id="limits-heading">{t('limits.heading')}</h2>
      <ul>
        {limited.map(({ feature, status, reasons }) => (
          <li key={feature}>
            <strong>{t(`limits.feature.${feature}`)}</strong>{' '}
            <span className="limits-status">{t(`limits.status.${status}`)}</span>
            {reasons.map((reason) => (
              <p key={reason}>{t(`limits.reason.${reason}`)}</p>
            ))}
            {feature === 'screenshots' && reasons.includes('consent_declined') && (
              <button type="button" onClick={onAskAgain}>
                {t('limits.askAgain')}
              </button>
            )}
          </li>
        ))}
      </ul>
    </section>
  );
}
