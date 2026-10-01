import type { TFunction } from 'i18next';
import type { Platform } from './bindings.gen';

/** "macOS", "Windows" or "Linux (Wayland)", in the member's language. */
export function describePlatform(platform: Platform, t: TFunction): string {
  const os = t(`about.os.${platform.os}`);
  return platform.linux_session === null
    ? os
    : t('about.linuxSession', { session: t(`about.session.${platform.linux_session}`) });
}
