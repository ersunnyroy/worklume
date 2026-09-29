import { QueryClient } from '@tanstack/react-query';

/**
 * One client per app instance. Error-aware retries (no retry on 4xx) arrive with the generated
 * API client in E00-S07, which knows the problem+json error shape.
 */
export function createQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: {
        staleTime: 30_000,
      },
    },
  });
}
