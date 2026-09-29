import type { components } from './schema.gen';

/** RFC 9457 problem details as the API returns them (`application/problem+json`). */
export type Problem = components['schemas']['Problem'];
export type FieldError = components['schemas']['FieldError'];

export function isProblem(value: unknown): value is Problem {
  if (typeof value !== 'object' || value === null) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate['type'] === 'string' &&
    typeof candidate['title'] === 'string' &&
    typeof candidate['status'] === 'number' &&
    typeof candidate['code'] === 'string' &&
    typeof candidate['request_id'] === 'string'
  );
}

/**
 * A failed API call. `code` is the API's stable error code, or `http_<status>` when the
 * response was not problem details (for example an HTML page from a proxy). `requestId` is what
 * users quote to support; the web app's error screen shows it.
 */
export class ApiError extends Error {
  override readonly name = 'ApiError';
  readonly status: number;
  readonly code: string;
  readonly requestId: string | null;
  readonly fieldErrors: readonly FieldError[];

  constructor(status: number, problem: Problem | null) {
    super(problem?.title ?? `The request failed with HTTP ${String(status)}`);
    this.status = status;
    this.code = problem?.code ?? `http_${String(status)}`;
    this.requestId = problem?.request_id ?? null;
    this.fieldErrors = problem?.errors ?? [];
  }
}
