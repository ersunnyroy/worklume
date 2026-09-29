import type { ExpoConfig } from 'expo/config';

/**
 * Native identity and build settings. Native projects (ios/, android/) are generated from this
 * file by `expo prebuild` and are never committed.
 */
const config: ExpoConfig = {
  name: 'Worklume',
  slug: 'worklume',
  scheme: 'worklume',
  version: '0.1.0',
  orientation: 'portrait',
  userInterfaceStyle: 'automatic',
  icon: './assets/icon.png',
  ios: {
    bundleIdentifier: 'com.worklume.mobile',
    supportsTablet: true,
  },
  android: {
    package: 'com.worklume.mobile',
    adaptiveIcon: {
      foregroundImage: './assets/adaptive-icon.png',
      backgroundColor: '#1f5f8b',
    },
    // Permissions are added only by the stories that need them (background location: E27).
    permissions: [],
    blockedPermissions: ['android.permission.SYSTEM_ALERT_WINDOW'],
  },
  plugins: ['expo-dev-client'],
};

export default config;
