import createClient, { type Middleware } from 'openapi-fetch';
import { ApiError, isProblem } from './errors';
import type { paths } from './schema.gen';

export interface ApiClientOptions {
  /** API origin from the app's runtime configuration, for example `https://api.worklume.com`. */
  readonly baseUrl: string;
  /**
   * Returns the current access token, or null when signed out. Tokens stay in memory; the web
   * sign-in flow (E02-S05) supplies this.
   */
  readonly getAccessToken?: () => string | null | Promise<string | null>;
  /** Replaces the global fetch, for tests or platform-specific networking. */
  readonly fetch?: typeof globalThis.fetch;
}

/** Creates a client whose paths, parameters and responses are typed from the contract. */
export function createApiClient(options: ApiClientOptions) {
  const client = createClient<paths>({
    baseUrl: options.baseUrl,
    headers: { Accept: 'application/json' },
    ...(options.fetch === undefined ? {} : { fetch: options.fetch }),
  });
  if (options.getAccessToken !== undefined) {
    client.use(bearerToken(options.getAccessToken));
  }
  return client;
}

export type ApiClient = ReturnType<typeof createApiClient>;

function bearerToken(getAccessToken: NonNullable<ApiClientOptions['getAccessToken']>): Middleware {
  return {
    async onRequest({ request }) {
      const token = await getAccessToken();
      if (token !== null && token !== '') {
        request.headers.set('Authorization', `Bearer ${token}`);
      }
      return request;
    },
  };
}

interface CallResult<T> {
  readonly data?: T;
  readonly error?: unknown;
  readonly response: Response;
}

/**
 * Waits for a call and returns its data, or throws an {@link ApiError} built from the problem
 * details. Use it where a failure should reach an error boundary, for example in a TanStack
 * Query function.
 */
export async function unwrap<T>(call: Promise<CallResult<T>>): Promise<T> {
  const result = await call;
  if (result.response.ok) {
    return result.data as T;
  }
  throw new ApiError(result.response.status, isProblem(result.error) ? result.error : null);
}
