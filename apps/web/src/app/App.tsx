import { QueryClientProvider, type QueryClient } from '@tanstack/react-query';
import { RouterProvider } from '@tanstack/react-router';
import { StrictMode } from 'react';
import type { RuntimeConfig } from '../config/runtime-config';
import { RuntimeConfigProvider } from '../config/runtime-config-context';
import type { AppRouter } from './router';

interface AppProps {
  readonly config: RuntimeConfig;
  readonly queryClient: QueryClient;
  readonly router: AppRouter;
}

export function App({ config, queryClient, router }: AppProps) {
  return (
    <StrictMode>
      <RuntimeConfigProvider config={config}>
        <QueryClientProvider client={queryClient}>
          <RouterProvider router={router} />
        </QueryClientProvider>
      </RuntimeConfigProvider>
    </StrictMode>
  );
}
