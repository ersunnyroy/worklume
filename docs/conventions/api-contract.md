# API contract decisions

Decisions for the contract pipeline (E00-S07, first part). The architecture makes the API
contract-first with OpenAPI 3.1 and Spectral; these notes cover how that is set up.

## Contract only for what is being built

`openapi.yaml` describes the endpoints that are implemented or in progress, not the whole catalog
in `docs/planning/phase-2-api.md`. Each story adds its endpoints to the contract first. Otherwise
the contract tests that compare API routes with the contract (after E00-S02) would fail on
hundreds of endpoints that don't exist yet. It starts with `/health` and `/ready` from E00-S02,
and the shared problem-details schema.

## Conventions as lint rules

The API catalog's conventions that a machine can check are Spectral rules (see the package
README). The tenancy rule matters most: no tenant operation may accept `organization_id` from
the client, in parameters or at any depth of a request body. That rule is the contract-level half
of "never trust client-supplied tenant IDs"; the API's policies and the cross-tenant test harness
cover the rest.

Rules are tested like the ESLint config: a compliant fixture must produce no results at all, and
each variant must produce exactly the expected rule. A rule that silently stops firing fails the
build.

## Typed client: openapi-typescript and openapi-fetch

- `openapi-typescript` generates types only, so the client adds no runtime weight for the
  schema. `openapi-fetch` adds about 6 KB, within the web app's startup budget.
- The generated file is committed and excluded from Prettier and ESLint. CI regenerates it
  during the build and fails if it changed (the generated-files check in `ci.yml`).
- The client's `build` task lists the contract file as an input (`$TURBO_ROOT$/…`), so editing
  the contract invalidates Turborepo's cache for the client.
- `openapi-typescript` declares TypeScript ^5 as a peer; it generates correctly with 6.0, so
  `peerDependencyRules` allows 6. The generated types are typechecked with 6.0 in CI.
- Runtime validation schemas (zod/mini) are generated later if a client needs to validate
  responses; the architecture lists them under `packages/`.

## Supply chain

Spectral depends on `@scarf/scarf`, whose install script only sends download analytics.
`allowBuilds` records that it may not run.

## Still to come in E00-S07

- Contract tests that compare the API's routes and responses with this file (needs E00-S02).
- Published reference documentation (E22-S01).
