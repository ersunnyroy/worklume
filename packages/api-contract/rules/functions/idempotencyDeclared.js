// @ts-check
/**
 * Spectral function: a POST operation declares how retries are made safe.
 * Modes follow the "Idempotency" column of the API catalog.
 */
const MODES = ['natural', 'key-required', 'key-optional', 'client-id'];

/**
 * @param {Record<string, unknown> | null | undefined} operation
 * @param {unknown} _options
 * @param {{ path: (string | number)[] }} context
 * @returns {{ message: string, path: (string | number)[] }[]}
 */
export default function idempotencyDeclared(operation, _options, context) {
  const mode = operation?.['x-worklume-idempotency'];
  if (typeof mode !== 'string' || !MODES.includes(mode)) {
    return [
      {
        message: `POST operations must set x-worklume-idempotency to one of: ${MODES.join(', ')}.`,
        path: [...context.path],
      },
    ];
  }
  if (mode !== 'key-required' && mode !== 'key-optional') {
    return [];
  }

  const parameters = Array.isArray(operation?.['parameters']) ? operation['parameters'] : [];
  const header = parameters.find(
    (parameter) =>
      typeof parameter === 'object' &&
      parameter !== null &&
      parameter.in === 'header' &&
      typeof parameter.name === 'string' &&
      parameter.name.toLowerCase() === 'idempotency-key',
  );
  if (header === undefined) {
    return [
      {
        message: `A ${mode} operation must declare the Idempotency-Key header parameter.`,
        path: [...context.path, 'parameters'],
      },
    ];
  }
  if (mode === 'key-required' && header.required !== true) {
    return [
      {
        message: 'A key-required operation must mark the Idempotency-Key header as required.',
        path: [...context.path, 'parameters'],
      },
    ];
  }
  return [];
}
