import { StatusBar } from 'expo-status-bar';
import { StyleSheet, Text, useColorScheme, View } from 'react-native';

// Same values as the web app, which pass WCAG AA contrast in its browser tests.
const palettes = {
  light: { canvas: '#ffffff', ink: '#1a1f24', muted: '#4a5560' },
  dark: { canvas: '#161b21', ink: '#e8ecef', muted: '#a9b4bf' },
} as const;

/**
 * Placeholder until the app shell and sign-in (E24-S01) replace it. Localisation arrives with
 * that story.
 */
export function App() {
  const colors = useColorScheme() === 'dark' ? palettes.dark : palettes.light;

  return (
    <View testID="app-screen" style={[styles.screen, { backgroundColor: colors.canvas }]}>
      <Text accessibilityRole="header" style={[styles.title, { color: colors.ink }]}>
        Worklume
      </Text>
      <Text style={[styles.body, { color: colors.muted }]}>
        Sign-in, time tracking and timesheets arrive in upcoming releases.
      </Text>
      <StatusBar style="auto" />
    </View>
  );
}

const styles = StyleSheet.create({
  screen: {
    flex: 1,
    justifyContent: 'center',
    paddingHorizontal: 24,
  },
  title: {
    fontSize: 28,
    fontWeight: '600',
    marginBottom: 12,
  },
  body: {
    fontSize: 16,
    lineHeight: 24,
  },
});
