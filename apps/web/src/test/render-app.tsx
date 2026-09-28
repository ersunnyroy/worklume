import { createMemoryHistory } from '@tanstack/react-router';
import { render, screen } from '@testing-library/react';
import { App } from '../app/App';
import { createQueryClient } from '../app/query-client';
import { createAppRouter } from '../app/router';
import { parseRuntimeConfig, type Environment } from '../config/runtime-config';

interface RenderAppOptions {
  readonly path?: string;
  readonly environment?: Environment;
}

/** Renders the whole app, as main.tsx does, at a path in memory history. */
export async function renderApp({ path = '/', environment = 'local' }: RenderAppOptions = {}) {
  const config = parseRuntimeConfig({
    environment,
    apiBaseUrl: environment === 'local' ? 'http://localhost:8000' : 'https://api.example.test',
  });
  const queryClient = createQueryClient();
  const router = createAppRouter(queryClient, createMemoryHistory({ initialEntries: [path] }));
  await router.load();
  const view = render(<App config={config} queryClient={queryClient} router={router} />);
  await screen.findByRole('main');
  return { ...view, router };
}
