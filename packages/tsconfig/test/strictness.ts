/**
 * Compile-time checks that the shared settings stay strict.
 * Each `@ts-expect-error` fails the typecheck if the error it expects
 * disappears, so loosening a flag in base.json breaks this build.
 */

const members: readonly string[] = ['a'];

// noUncheckedIndexedAccess: indexing may yield undefined.
// @ts-expect-error string | undefined is not assignable to string
const first: string = members[0];

interface Options {
  readonly retries?: number;
}

// exactOptionalPropertyTypes: undefined is not the same as "absent".
// @ts-expect-error undefined is not assignable to an optional number
const options: Options = { retries: undefined };

const counts: Record<string, number> = {};
// noPropertyAccessFromIndexSignature: index signatures need bracket access.
// @ts-expect-error property comes from an index signature
const total = counts.total;

function parse(input: string): number {
  try {
    return JSON.parse(input) as number;
  } catch (error) {
    // useUnknownInCatchVariables: the caught value is unknown.
    // @ts-expect-error 'error' is of type 'unknown'
    return error.code as number;
  }
}

export { first, options, total, parse };
