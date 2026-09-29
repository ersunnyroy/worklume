import { defineConfig, devices } from '@playwright/test';

const port = 4173;
const isCI = Boolean(process.env['CI']);
// Lets environments with a preinstalled Chromium (such as sandboxes) skip the browser download.
const chromiumPath = process.env['PLAYWRIGHT_CHROMIUM_EXECUTABLE'];
const launchOptions =
  chromiumPath === undefined ? {} : { launchOptions: { executablePath: chromiumPath } };

/** Smoke tests against the production build, served the way S3 and CloudFront serve it. */
export default defineConfig({
  testDir: './e2e',
  fullyParallel: true,
  forbidOnly: isCI,
  retries: isCI ? 1 : 0,
  reporter: isCI ? [['list'], ['html', { open: 'never' }]] : 'list',
  use: {
    baseURL: `http://localhost:${String(port)}`,
    trace: 'retain-on-failure',
  },
  projects: [
    { name: 'desktop', use: { ...devices['Desktop Chrome'], ...launchOptions } },
    { name: 'mobile', use: { ...devices['Pixel 7'], ...launchOptions } },
  ],
  webServer: {
    // Run Vite directly: a package-manager wrapper can outlive the kill signal and hang teardown.
    command: 'vite preview',
    url: `http://localhost:${String(port)}`,
    reuseExistingServer: !isCI,
  },
});
