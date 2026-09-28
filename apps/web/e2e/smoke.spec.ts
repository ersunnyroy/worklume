import { AxeBuilder } from '@axe-core/playwright';
import { expect, test } from '@playwright/test';

test('sends the production security headers', async ({ page }) => {
  const response = await page.goto('/');
  const headers = response?.headers() ?? {};

  expect(headers['content-security-policy']).toContain("script-src 'self'");
  expect(headers['content-security-policy']).toContain("frame-ancestors 'none'");
  expect(headers['x-content-type-options']).toBe('nosniff');
});

test('loads the home page without console errors', async ({ page }) => {
  const errors: string[] = [];
  page.on('console', (message) => {
    if (message.type() === 'error') {
      errors.push(message.text());
    }
  });
  page.on('pageerror', (error) => errors.push(error.message));

  await page.goto('/');

  await expect(page.getByRole('heading', { level: 1, name: 'Welcome to Worklume' })).toBeVisible();
  await expect(page).toHaveTitle('Home · Worklume');
  await expect(page.getByText('Local environment')).toBeVisible();
  expect(errors).toEqual([]);
});

test('serves deep links and shows not found for unknown pages', async ({ page }) => {
  await page.goto('/reports/weekly/unknown');

  await expect(page.getByRole('heading', { level: 1, name: 'Page not found' })).toBeVisible();
  await page.getByRole('link', { name: 'Go to home' }).click();
  await expect(page).toHaveURL('/');
});

for (const colorScheme of ['light', 'dark'] as const) {
  test(`passes WCAG 2.2 AA automated checks in ${colorScheme} mode`, async ({ page }) => {
    await page.emulateMedia({ colorScheme });
    await page.goto('/');
    await expect(page.getByRole('main')).toBeVisible();

    const results = await new AxeBuilder({ page })
      .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa'])
      .analyze();

    expect(results.violations.map((violation) => violation.id)).toEqual([]);
  });
}

test('keyboard users reach the skip link first and land in the main content', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('main')).toBeVisible();

  await page.keyboard.press('Tab');
  const skipLink = page.getByRole('link', { name: 'Skip to main content' });
  await expect(skipLink).toBeFocused();
  await expect(skipLink).toBeVisible();

  await page.keyboard.press('Enter');
  await expect(page.getByRole('main')).toBeFocused();
});

test('shows a startup error when config.json is unavailable', async ({ page }) => {
  await page.route('**/config.json', (route) => route.fulfill({ status: 503, body: 'down' }));

  await page.goto('/');

  await expect(page.getByRole('heading', { name: "Worklume couldn't start" })).toBeVisible();
});

test('uses a navigation drawer on small screens', async ({ page, isMobile }) => {
  const errors: string[] = [];
  page.on('console', (message) => {
    if (message.type() === 'error') {
      errors.push(message.text());
    }
  });
  await page.goto('/');
  const menuButton = page.getByRole('button', { name: 'Open navigation' });

  if (!isMobile) {
    await expect(menuButton).toBeHidden();
    await expect(page.getByRole('navigation', { name: 'Primary' })).toBeVisible();
    return;
  }

  await expect(page.getByRole('navigation', { name: 'Primary' })).toBeHidden();
  await menuButton.click();
  const drawer = page.getByRole('dialog', { name: 'Navigation' });
  await expect(drawer).toBeVisible();
  await drawer.getByRole('link', { name: 'Home' }).click();
  await expect(drawer).toBeHidden();
  // Content-Security-Policy violations are reported as console errors.
  expect(errors).toEqual([]);
});
