import { useTranslation } from 'react-i18next';
import { isReduced, type DesktopCapabilities } from './capabilities';

/**
 * "Wayland: reduced" and the like, for a device whose agent can't measure everything. Nothing
 * when it measures everything, or before its agent has reported.
 */
export function DeviceCapabilityBadge({
  capabilities,
}: {
  readonly capabilities: DesktopCapabilities | null;
}) {
  const { t } = useTranslation();
  if (capabilities === null || !isReduced(capabilities)) {
    return null;
  }
  return (
    <span className="rounded-full bg-warning px-2.5 py-0.5 text-xs font-medium text-warning-ink">
      {t('devices.capabilities.badge', {
        platform: t(`devices.capabilities.platform.${capabilities.platform}`),
      })}
    </span>
  );
}
