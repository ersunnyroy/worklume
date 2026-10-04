import { useId } from 'react';
import { useTranslation } from 'react-i18next';
import { limitedFeatures, type DesktopCapabilities } from './capabilities';

/**
 * Why a device's agent can't fully measure some features, so a member's missing screenshots
 * or apps read as a limit of their desktop rather than as gaps. Nothing when it measures
 * everything, or before its agent has reported.
 */
export function DeviceCapabilityDetails({
  capabilities,
}: {
  readonly capabilities: DesktopCapabilities | null;
}) {
  const { t } = useTranslation();
  const headingId = useId();
  const limited = capabilities === null ? [] : limitedFeatures(capabilities);
  if (limited.length === 0) {
    return null;
  }
  return (
    <section aria-labelledby={headingId} className="rounded-lg border border-line bg-surface p-4">
      <h2 id={headingId} className="text-base font-semibold">
        {t('devices.capabilities.heading')}
      </h2>
      <ul className="mt-2 grid gap-3">
        {limited.map(({ feature, status, reasons }) => (
          <li key={feature}>
            <span className="font-medium">{t(`devices.capabilities.feature.${feature}`)}</span>{' '}
            <span className="text-muted">{t(`devices.capabilities.status.${status}`)}</span>
            {reasons.map((reason) => (
              <p key={reason} className="mt-1 text-sm text-muted">
                {t(`devices.capabilities.reason.${reason}`)}
              </p>
            ))}
          </li>
        ))}
      </ul>
    </section>
  );
}
