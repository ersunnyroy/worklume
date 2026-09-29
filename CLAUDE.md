# Working in this repository

Worklume is a multi-tenant time-tracking and workforce platform. The documents in
`docs/planning/` are authoritative, in this order:

1. `phase-1-research.md`: feature research and release scope.
2. `phase-2-architecture.md`, `phase-2-database.md`, `phase-2-api.md`: the approved
   architecture, data model and API.
3. `phase-3-plan.md`, `phase-3-epics-and-stories.md`: the implementation plan and backlog.

Do not redesign the architecture unless a concrete technical problem requires it; record any
such change as a decision in `docs/conventions/` and in the pull request.

## Before changing code

Inspect the existing code and the relevant plan sections, then identify the affected modules,
database and API dependencies, authorization and tenant-isolation requirements, and the tests
needed. Write the plan before modifying code. Implement one story per change.

## Loop

Research → plan → implement → test → security review → tenant-isolation review →
performance review → document → commit. Tests are never skipped.

## Rules

- **Tenancy:** derive tenant context from the authenticated session. Never trust
  `organization_id`, `user_id`, `project_id` or `task_id` from a client; verify every
  relationship. Every feature ships tenant-isolation tests.
- **API:** every endpoint defines authentication, authorization, validation, rate limiting
  where appropriate, problem+json errors, logging, idempotency where it has side effects, and
  tests. Describe it in `packages/api-contract/openapi.yaml` first; Spectral enforces the
  conventions and the client is generated from it.
- **Database:** foreign keys, indexes, unique and check constraints, transactions and the
  right isolation level. Integrity is enforced in the database, not only in code.
- **Desktop agent:** works offline, recovers from crashes, queues and retries events safely
  without duplicates, encrypts sensitive local data, protects tokens, supports auto-update,
  logs failures without personal data. Platform-specific code stays in the per-OS adapters.
- **Sensitive data:** screenshots, activity, application and website usage, location and
  employee data are never exposed without an authorization policy.
- **Avoid:** large controllers or components, business logic in controllers or UI,
  unnecessary repositories or services, hardcoded configuration, secrets in source.

## Report after each story

1. What was implemented
2. Files changed
3. Database changes
4. APIs added or changed
5. Tests added
6. Security considerations
7. Tenant-isolation considerations
8. Remaining work
9. Potential risks

## Commands

`pnpm install`, then `pnpm check` (format, lint, typecheck, test, build) and `pnpm test:e2e`
(browser smoke tests). See `README.md` and `CONTRIBUTING.md` for the rest.
