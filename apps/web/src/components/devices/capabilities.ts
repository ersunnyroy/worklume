import type { components } from '@worklume/api-client';

/** What a device's agent reported it can measure (`PUT /v1/desktop/capabilities`). */
export type DesktopCapabilities = components['schemas']['DesktopCapabilities'];
export type DeviceFeature = keyof DesktopCapabilities['features'];
export type DeviceCapability = components['schemas']['DesktopCapability'];

/** The features in the order the dashboard lists them. */
export const DEVICE_FEATURES: readonly DeviceFeature[] = [
  'screenshots',
  'activity',
  'idle',
  'apps',
  'urls',
  'session_lock',
];

/** The features the device doesn't measure fully, in display order. */
export function limitedFeatures(
  capabilities: DesktopCapabilities,
): readonly (DeviceCapability & { readonly feature: DeviceFeature })[] {
  return DEVICE_FEATURES.map((feature) => ({ feature, ...capabilities.features[feature] })).filter(
    (capability) => capability.status !== 'granted',
  );
}

/** A device is reduced when any feature isn't granted: "Wayland: reduced". */
export function isReduced(capabilities: DesktopCapabilities): boolean {
  return limitedFeatures(capabilities).length > 0;
}
