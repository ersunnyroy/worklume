## Story

<!-- Story ID from the implementation plan, e.g. E03-S04, and a one-line summary. -->

Refs:

## What changed

<!-- The behaviour change, in a few lines. Link the plan section if it helps. -->

## How it was tested

<!-- Commands run, and what each new test proves. -->

## Definition of Done

Tick each line or write "N/A" with the reason. Unexplained blanks block review.

- [ ] Code lives in the owning domain module; no cross-module internals (architecture tests green)
- [ ] Migration complete: indexes, composite FKs, RLS policy, grants; expand/contract safe
- [ ] Authorization: policy on every action; new permissions in the catalog
- [ ] Tenant isolation tested: new routes, jobs, cache keys, files and channels are in the cross-tenant harness
- [ ] Client-supplied IDs (organization, user, project, task) are never trusted; tenant context comes from the session
- [ ] API documented in OpenAPI with examples, error codes and idempotency
- [ ] Frontend accessible (WCAG 2.2 AA) and localisable
- [ ] Desktop and mobile behaviour covered where the story needs it
- [ ] Tests: unit, feature/API, and E2E for user-facing flows; new domain logic ≥ 90% covered
- [ ] Errors use problem+json codes; side effects are idempotent
- [ ] Structured logs carry request_id and org_id, with no personal data or secrets
- [ ] Metrics, traces and alerts for new jobs or endpoints
- [ ] Audit events for security-relevant and admin actions
- [ ] Security review: threat notes for auth, money, files and external calls; scans clean
- [ ] Privacy review: data class, retention, notice impact, employee visibility
- [ ] Performance within the p95 budget for hot paths
- [ ] Feature flag and rollback step for risky changes
- [ ] Docs updated (user help, admin docs, API changelog, runbook)

## Reviews

- **Security:**
- **Tenant isolation:**
- **Performance:**
