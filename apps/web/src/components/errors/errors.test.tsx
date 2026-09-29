import {
  createRootRoute,
  createRouter,
  RouterProvider,
  createMemoryHistory,
} from '@tanstack/react-router';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { renderApp } from '../../test/render-app';
import { errorReference, RouteError } from './RouteError';

describe('not found', () => {
  it('renders inside the shell with a way home', async () => {
    const { router } = await renderApp({ path: '/no/such/page' });

    expect(screen.getByRole('heading', { level: 1, name: 'Page not found' })).toBeVisible();
    expect(screen.getByRole('navigation', { name: 'Primary' })).toBeInTheDocument();
    expect(document.title).toBe('Page not found · Worklume');

    await userEvent.setup().click(screen.getByRole('link', { name: 'Go to home' }));
    expect(await screen.findByRole('heading', { name: 'Welcome to Worklume' })).toBeVisible();
    expect(router.state.location.pathname).toBe('/');
  });
});

describe('route error', () => {
  function renderFailingRoute(error: unknown) {
    // React and the router log the thrown error; the assertions below cover what users see.
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const onRender = vi.fn(() => {
      throw error;
    });
    const rootRoute = createRootRoute({ component: onRender, errorComponent: RouteError });
    const router = createRouter({
      routeTree: rootRoute,
      history: createMemoryHistory({ initialEntries: ['/'] }),
    });
    render(<RouterProvider router={router} />);
    return onRender;
  }

  it('shows a generic message with a retry button', async () => {
    vi.stubEnv('DEV', false);
    const onRender = renderFailingRoute(new Error('SQLSTATE[42P01]: relation "users" missing'));

    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('Something went wrong');
    expect(alert).not.toHaveTextContent('SQLSTATE');
    const callsBeforeRetry = onRender.mock.calls.length;

    await userEvent.setup().click(screen.getByRole('button', { name: 'Try again' }));
    expect(onRender.mock.calls.length).toBeGreaterThan(callsBeforeRetry);
  });

  it('shows the support reference when the error carries one', async () => {
    renderFailingRoute(Object.assign(new Error('Server error'), { requestId: 'req_0192f0a4' }));

    expect(await screen.findByText('Reference: req_0192f0a4')).toBeInTheDocument();
  });

  it('shows the error message only in development builds', async () => {
    vi.stubEnv('DEV', true);
    renderFailingRoute(new Error('Cannot read properties of undefined'));

    expect(await screen.findByRole('alert')).toHaveTextContent('Cannot read properties');
  });
});

describe('errorReference', () => {
  it.each([
    [{ requestId: 'req_1' }, 'req_1'],
    [{ requestId: '' }, null],
    [{ requestId: 42 }, null],
    [new Error('plain'), null],
    ['string error', null],
    [null, null],
  ])('reads %j as %s', (error, expected) => {
    expect(errorReference(error)).toBe(expected);
  });
});
