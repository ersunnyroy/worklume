# API contract decisions

Decisions for the contract pipeline (E00-S07, first part). The architecture makes the API
contract-first with OpenAPI 3.1 and Spectral; these notes cover how that is set up.

## Contract only for what is being built

`openapi.yaml` describes the endpoints that are implemented or in progress, not the whole catalog
in `docs/planning/phase-2-api.md`. Each story adds its endpoints to the contract first. Otherwise
the contract tests that compare API routes with the contract (after E00-S02) would fail on
hundreds of endpoints that don't exist yet. It starts with `/health` and `/ready` from E00-S02,
and the shared problem-details schema.

## Desktop events: agent and contract in step

`POST /v1/desktop/sync` and its `DesktopEvent` schemas came with the tracker (E09-S04), which
produces the events. The API side arrives with E07-S03.

- **Envelope:** `event_id`, `seq`, `type`, `wall_time`, `mono_time`, `clock_offset_ms` and
  `payload`, from architecture §13.
- **Variants:** each event type is its own `allOf` over the envelope, with
  `unevaluatedProperties: false`, so neither the envelope nor the payload accepts unknown fields.
- **Server obligations:** each event schema's description states what the server must do with
  it.

Drift is caught from both sides:

- `apps/desktop/sync/tests/contract_fixture.rs` runs the real tracker through a scenario that
  produces every event type. It encodes the events as the sync client will, and compares them
  with `test/fixtures/desktop-events.json`, with IDs replaced by placeholders.
  `UPDATE_FIXTURES=1` rewrites the fixture.
- `test/desktop-events.test.js` validates every fixture event against the schema with Ajv
  (JSON Schema 2020-12, with formats). It requires every schema type to appear in the fixture,
  and checks that missing, extra and malformed fields are refused.
- Changing an event without the contract fails the Rust test; changing the contract without the
  agent fails the JavaScript test. The desktop package's Turborepo test inputs include the
  fixture.

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
