import { describe, expect, it, vi } from 'vitest';
import { ApiError, createApiClient, isProblem, unwrap, type Problem } from '../src';

const baseUrl = 'https://api.example.test';

function respond(status: number, body: unknown, contentType = 'application/json') {
  return vi.fn<typeof fetch>(() =>
    Promise.resolve(
      new Response(typeof body === 'string' ? body : JSON.stringify(body), {
        status,
        headers: { 'Content-Type': contentType },
      }),
    ),
  );
}

function sentRequest(fetchMock: ReturnType<typeof respond>): Request {
  const [request] = fetchMock.mock.calls[0] ?? [];
  if (!(request instanceof Request)) {
    throw new Error('fetch was not called with a Request');
  }
  return request;
}

const unavailable: Problem = {
  type: 'https://docs.worklume.com/errors/service_unavailable',
  title: 'Service unavailable',
  status: 503,
  code: 'service_unavailable',
  request_id: '0192f0a4-7c1e-7d3a-9b2f-5d8e6a4c3b21',
};

describe('createApiClient', () => {
  it('returns typed data for a successful call', async () => {
    const fetchMock = respond(200, { status: 'ok' });
    const api = createApiClient({ baseUrl, fetch: fetchMock });

    const health = await unwrap(api.GET('/health'));

    expect(health.status).toBe('ok');
    expect(sentRequest(fetchMock).url).toBe(`${baseUrl}/health`);
  });

  it('turns problem details into an ApiError with the code and request ID', async () => {
    const api = createApiClient({
      baseUrl,
      fetch: respond(503, unavailable, 'application/problem+json'),
    });

    const error = await unwrap(api.GET('/ready')).catch((caught: unknown) => caught);

    expect(error).toBeInstanceOf(ApiError);
    expect(error).toMatchObject({
      status: 503,
      code: 'service_unavailable',
      requestId: unavailable.request_id,
      message: 'Service unavailable',
    });
  });

  it('reports a non-problem error response by its HTTP status', async () => {
    const api = createApiClient({
      baseUrl,
      fetch: respond(502, '<html>Bad gateway</html>', 'text/html'),
    });

    const error = await unwrap(api.GET('/ready')).catch((caught: unknown) => caught);

    expect(error).toMatchObject({ status: 502, code: 'http_502', requestId: null });
  });

  it('sends the access token when there is one', async () => {
    const fetchMock = respond(200, { status: 'ok' });
    const api = createApiClient({ baseUrl, fetch: fetchMock, getAccessToken: () => 'token-123' });

    await api.GET('/health');

    expect(sentRequest(fetchMock).headers.get('Authorization')).toBe('Bearer token-123');
  });

  it('sends no Authorization header when signed out', async () => {
    const fetchMock = respond(200, { status: 'ok' });
    const api = createApiClient({ baseUrl, fetch: fetchMock, getAccessToken: () => null });

    await api.GET('/health');

    expect(sentRequest(fetchMock).headers.has('Authorization')).toBe(false);
  });

  it('only knows the paths in the contract', () => {
    const api = createApiClient({ baseUrl, fetch: respond(200, {}) });
    // @ts-expect-error '/v1/unknown' is not in the contract, so this does not compile.
    void api.GET('/v1/unknown');
  });
});

describe('isProblem', () => {
  it.each([
    [unavailable, true],
    [{ ...unavailable, request_id: undefined }, false],
    [{ message: 'Server error' }, false],
    ['Bad gateway', false],
    [null, false],
  ])('recognises %j as %s', (value, expected) => {
    expect(isProblem(value)).toBe(expected);
  });
});
