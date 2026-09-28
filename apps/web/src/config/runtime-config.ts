// zod/mini keeps validation out of the startup bundle's critical weight (about 5 KB gzip,
// against about 60 KB for the classic API).
import * as z from 'zod/mini';

/**
 * Settings that differ between environments. They are read at startup from `/config.json`,
 * which each deployment writes next to the built app, so one build can be promoted from
 * staging to production unchanged. Nothing secret belongs here: the file is public.
 */
const environments = ['local', 'development', 'staging', 'production'] as const;

function isPlainBaseUrl(value: string): boolean {
  const url = new URL(value);
  return url.username === '' && url.password === '' && url.search === '' && url.hash === '';
}

const runtimeConfigSchema = z.pipe(
  z
    .strictObject({
      environment: z.enum(environments),
      apiBaseUrl: z
        .url({ protocol: /^https?$/, hostname: z.regexes.hostname })
        .check(z.refine(isPlainBaseUrl, 'must not contain credentials, a query or a fragment')),
    })
    .check(
      z.refine(
        (config) => config.environment === 'local' || config.apiBaseUrl.startsWith('https://'),
        { message: 'must use https outside local development', path: ['apiBaseUrl'] },
      ),
    ),
  z.transform((config) => ({ ...config, apiBaseUrl: config.apiBaseUrl.replace(/\/+$/, '') })),
);

export type RuntimeConfig = Readonly<z.output<typeof runtimeConfigSchema>>;
export type Environment = RuntimeConfig['environment'];

export class RuntimeConfigError extends Error {
  override readonly name = 'RuntimeConfigError';
}

export function parseRuntimeConfig(input: unknown): RuntimeConfig {
  const result = runtimeConfigSchema.safeParse(input);
  if (!result.success) {
    throw new RuntimeConfigError(`Invalid config.json:\n${z.prettifyError(result.error)}`);
  }
  return Object.freeze(result.data);
}

export async function loadRuntimeConfig(fetchConfig: typeof fetch = fetch): Promise<RuntimeConfig> {
  const response = await fetchConfig('/config.json', {
    cache: 'no-store',
    headers: { Accept: 'application/json' },
  });
  if (!response.ok) {
    throw new RuntimeConfigError(`config.json request failed with HTTP ${response.status}`);
  }
  return parseRuntimeConfig(await response.json());
}
