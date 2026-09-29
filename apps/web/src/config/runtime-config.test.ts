import { describe, expect, it, vi } from 'vitest';
import { loadRuntimeConfig, parseRuntimeConfig, RuntimeConfigError } from './runtime-config';

describe('parseRuntimeConfig', () => {
  it('accepts a local config and trims the trailing slash from the API URL', () => {
    const config = parseRuntimeConfig({
      environment: 'local',
      apiBaseUrl: 'http://localhost:8000/',
    });
    expect(config).toEqual({ environment: 'local', apiBaseUrl: 'http://localhost:8000' });
  });

  it('returns a frozen object', () => {
    const config = parseRuntimeConfig({
      environment: 'production',
      apiBaseUrl: 'https://api.worklume.com',
    });
    expect(Object.isFrozen(config)).toBe(true);
  });

  it.each(['development', 'staging', 'production'])('requires https in %s', (environment) => {
    expect(() =>
      parseRuntimeConfig({ environment, apiBaseUrl: 'http://api.worklume.com' }),
    ).toThrow(/must use https/);
  });

  it.each([
    ['an unknown environment', { environment: 'qa', apiBaseUrl: 'https://api.worklume.com' }],
    ['a missing API URL', { environment: 'production' }],
    ['a non-http scheme', { environment: 'local', apiBaseUrl: 'javascript:alert(1)' }],
    [
      'credentials in the URL',
      { environment: 'local', apiBaseUrl: 'https://a:b@api.worklume.com' },
    ],
    ['a query string', { environment: 'local', apiBaseUrl: 'https://api.worklume.com/?x=1' }],
    [
      'unknown keys',
      { environment: 'local', apiBaseUrl: 'https://api.worklume.com', apiSecret: 'nope' },
    ],
    ['a non-object', 'production'],
  ])('rejects %s', (_case, input) => {
    expect(() => parseRuntimeConfig(input)).toThrow(RuntimeConfigError);
  });
});

describe('loadRuntimeConfig', () => {
  it('fetches /config.json without caching', async () => {
    const fetchConfig = vi.fn<typeof fetch>(() =>
      Promise.resolve(
        Response.json({ environment: 'staging', apiBaseUrl: 'https://api.staging.worklume.com' }),
      ),
    );

    const config = await loadRuntimeConfig(fetchConfig);

    expect(config.environment).toBe('staging');
    expect(fetchConfig).toHaveBeenCalledWith(
      '/config.json',
      expect.objectContaining({ cache: 'no-store' }),
    );
  });

  it('fails with a config error when the file is missing', async () => {
    const fetchConfig = vi.fn<typeof fetch>(() =>
      Promise.resolve(new Response('Not found', { status: 404 })),
    );

    await expect(loadRuntimeConfig(fetchConfig)).rejects.toThrow(/HTTP 404/);
  });
});
