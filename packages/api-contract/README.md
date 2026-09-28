# @worklume/api-contract

The Worklume API contract, in OpenAPI 3.1 (`openapi.yaml`). It is the source of truth: the
Laravel API implements it, `@worklume/api-client` is generated from it, and CI fails when either
drifts.

## Changing the API

1. Describe the change here first: paths, parameters, request and response schemas, error codes,
   and for every POST its idempotency mode (`x-worklume-idempotency`).
2. Run `pnpm --filter @worklume/api-contract lint`.
3. Run `pnpm --filter @worklume/api-client build` to regenerate the client types, and commit both.
4. Implement the endpoint in the API. Once the API exists (E00-S02), contract tests compare its
   routes and responses with this file.

## Rules

`.spectral.yaml` runs Spectral's OpenAPI recommendations plus Worklume's conventions:

| Rule                                           | Enforces                                                                 |
| ---------------------------------------------- | ------------------------------------------------------------------------ |
| `worklume-no-client-organization-id-parameter` | No `organization_id` parameter; the organization comes from the token    |
| `worklume-no-client-organization-id-body`      | No `organization_id` anywhere in a request body                          |
| `worklume-snake-case-properties`               | JSON properties are `snake_case`                                         |
| `worklume-problem-json-errors`                 | 4xx, 5xx and default responses are `application/problem+json` only       |
| `worklume-versioned-paths`                     | Paths start with `/v1/`                                                  |
| `worklume-path-casing`                         | Kebab-case segments, `snake_case` path parameters                        |
| `worklume-operation-id-camel-case`             | camelCase operation IDs (they become client method names)                |
| `worklume-idempotency-declared`                | Every POST declares its idempotency; key modes declare `Idempotency-Key` |

Two extensions opt out, and each use is visible in review:

- `x-worklume-unversioned: true` on a path item, for health checks and standard well-known
  endpoints.
- `x-worklume-allows-organization-id: true` on an operation, only for identity-level operations
  that choose an organization, such as `POST /v1/auth/switch-organization`. The API must still
  check that the caller is a member of that organization.

`pnpm --filter @worklume/api-contract test` lints a compliant fixture and one variant per rule,
and checks that exactly the expected rule fires.
