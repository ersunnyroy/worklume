import { screen, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { accessibilityViolations } from '../../test/axe';
import { renderApp } from '../../test/render-app';

describe('app shell', () => {
  it('renders the landmarks and the home page', async () => {
    await renderApp();

    expect(screen.getByRole('banner')).toBeInTheDocument();
    expect(screen.getByRole('navigation', { name: 'Primary' })).toBeInTheDocument();
    expect(screen.getByRole('main')).toHaveAttribute('id', 'main');
    expect(screen.getByRole('heading', { level: 1, name: 'Welcome to Worklume' })).toBeVisible();
  });

  it('titles the page', async () => {
    await renderApp();

    expect(document.title).toBe('Home · Worklume');
  });

  it('offers a skip link to the main content', async () => {
    await renderApp();

    const skipLink = screen.getByRole('link', { name: 'Skip to main content' });
    expect(skipLink).toHaveAttribute('href', '#main');
  });

  it('marks the current page in the navigation', async () => {
    await renderApp();

    const nav = screen.getByRole('navigation', { name: 'Primary' });
    expect(within(nav).getByRole('link', { name: 'Home' })).toHaveAttribute('aria-current', 'page');
  });

  it('labels non-production environments', async () => {
    await renderApp({ environment: 'staging' });

    expect(screen.getByText('Staging environment')).toBeInTheDocument();
  });

  it('shows no environment label in production', async () => {
    await renderApp({ environment: 'production' });

    expect(screen.queryByText(/environment$/)).not.toBeInTheDocument();
  });

  it('has no detectable accessibility violations', async () => {
    await renderApp();

    expect(await accessibilityViolations()).toEqual([]);
  });
});
