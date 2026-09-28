# Epics and stories

36 epics, each with its purpose, dependencies, features, tables, APIs, client work and tests, followed by its stories.

## How to read this tab

Every epic has a card (purpose, dependencies, features, tables, APIs, client work, tests, release) and a story table. Story IDs are `E<epic>-S<nn>`. Tables and API paths refer to the Phase 2 Database and API tabs.

| Size | Meaning (complexity, not time) |
| --- | --- |
| XS | A trivial change in one place; no new concepts |
| S | One layer (e.g. an endpoint or a screen) using existing patterns |
| M | One feature across database, API and UI, using existing patterns |
| L | A feature that introduces a new pattern, new infrastructure or tricky rules |
| XL | Cross-domain work, OS-native code, security-critical logic or money movement |
| XXL | Too big for one sprint — must be split before it is started |

**Apps** column: API (Laravel), Web, Desktop, Mobile, Ext (browser extension), AI (AI plane), Infra.

**Every story** also carries the standard tasks and the Definition of Done from the Plan tab; acceptance criteria below list only what is specific to the story.

## E00 · Engineering foundation

| Field | Value |
| --- | --- |
| Purpose | A working monorepo, environments and pipelines so every later epic ships the same way |
| Depends on | — |
| Features | Monorepo and tooling; Laravel module skeleton; AWS infrastructure as code; CI; observability; local dev; OpenAPI contract; web shell |
| Tables | Framework tables only (`migrations`, `failed_jobs`, `job_batches`) |
| APIs | `GET /health`, `GET /ready` |
| Frontend | Vite SPA shell, routing, design tokens, i18n |
| Desktop | Tauri 2 project scaffold in `apps/desktop` (builds an empty tray app on all 3 OSes) |
| Mobile | Expo project scaffold (builds only) |
| Tests | Architecture tests (module boundaries); pipeline smoke tests; Terraform plan checks |
| Release | Alpha |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E00-S01 | Monorepo and tooling | pnpm + Turborepo for TS, Composer for PHP, Cargo workspace for Rust. AC: one command builds all apps from a fresh clone; shared lint configs; CODEOWNERS | — | M | — | — | All |
| E00-S02 | Laravel modular skeleton | Laravel 12, PHP 8.4, Octane (FrankenPHP), `app/Domains/*` modules, PHPStan max, Pint, Pest, deptrac rules. AC: a test fails when a module imports another's internals | S01 | M | framework | health | API |
| E00-S03 | Infrastructure as code | Terraform for accounts, VPC, ECS, RDS PostgreSQL 17, ElastiCache, S3 buckets, KMS, Secrets Manager, CloudFront, WAF for dev and staging. AC: `terraform apply` builds a working environment; no manual console changes | — | L | — | — | Infra |
| E00-S04 | CI pipelines | GitHub Actions: lint, static analysis, tests, image build, deploy to dev on merge. AC: PR checks under 15 minutes; failed check blocks merge | S01–S03 | M | — | — | Infra |
| E00-S05 | Observability baseline | OpenTelemetry traces, JSON logs with `request_id` and `org_id`, Sentry for PHP and web, Grafana dashboards, on-call alerts. AC: a request is traceable end to end from its ID | S02, S03 | M | — | — | API, Web, Infra |
| E00-S06 | Local dev environment | Docker Compose with PostgreSQL 17 + extensions, Valkey, MinIO, Mailpit. AC: new engineer productive in under an hour | S02 | S | — | — | All |
| E00-S07 | OpenAPI contract pipeline | OpenAPI 3.1 spec, Spectral lint, generated TS client package, contract tests comparing routes with the spec. AC: CI fails on drift | S02 | M | — | — | API, Web |
| E00-S08 | Web app shell | Vite, TanStack Router/Query, Tailwind + Radix, layout, error boundary, i18n. AC: deployable to S3 + CloudFront | S01 | M | — | — | Web |
| E00-S09 | Desktop and mobile scaffolds | Tauri 2 tray app and Expo app that build in CI (unsigned). AC: artifacts for macOS, Windows, Ubuntu, iOS simulator, Android | S01 | S | — | — | Desktop, Mobile |

## E01 · Platform kernel

| Field | Value |
| --- | --- |
| Purpose | Cross-cutting mechanisms every domain relies on |
| Depends on | E00 |
| Features | Error format; transactional outbox; audit log; idempotency; jobs; feature flags; list conventions; rate limits |
| Tables | `outbox_events`, `audit_logs`, `idempotency_keys`, `feature_flags` |
| APIs | Behaviour shared by all endpoints; `GET /v1/jobs/{id}` |
| Frontend | Error toasts and problem+json handling; list and pagination components |
| Desktop / Mobile | Client handling of error codes and `Retry-After` |
| Tests | Outbox exactly-once-per-commit tests; audit hash-chain verification; idempotency replay tests |
| Release | Alpha |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E01-S01 | Problem+json errors and request IDs | Exception handler maps domain errors to RFC 9457 codes. AC: every error has `code` and `request_id`; no stack traces outside dev | E00 | S | — | all | API |
| E01-S02 | Transactional outbox and relay | Events written in the same transaction as the change; relay publishes to queues, Reverb and webhook dispatch. AC: nothing is published for a rolled-back transaction; at-least-once delivery; order kept per aggregate | E00 | L | outbox\_events | — | API |
| E01-S03 | Audit log service | Append-only writes with per-org hash chain, redaction rules, insert-only DB role, nightly anchor to an Object Lock bucket. AC: a tampered row fails verification | S02 | L | audit\_logs | — | API, Infra |
| E01-S04 | Idempotency keys | Middleware storing responses for 24 h; rejects same key + different body. AC: concurrent duplicates return one result | E00 | M | idempotency\_keys | all POST | API |
| E01-S05 | Job framework | Horizon queues, base job carrying `organization_id`, `PlatformJob` marker, dead-letter handling. AC: a tenant job without an org is rejected | E00 | M | — | /v1/jobs/{id} | API |
| E01-S06 | Feature flags | Laravel Pennant backed by `feature_flags`; per-org targeting. AC: flag checks cached per request | E00 | S | feature\_flags | — | API, Web |
| E01-S07 | List and concurrency conventions | Cursor pagination, `filter[]`, `sort`, `include`, ETag/If-Match helpers. AC: shared package used by all list endpoints | E00 | M | — | all lists | API, Web |
| E01-S08 | Rate limiting | Token-bucket limits per token, org, IP and device; `RateLimit-*` headers. AC: limits configurable per route group | E00 | M | — | all | API |

## E02 · Identity and authentication

| Field | Value |
| --- | --- |
| Purpose | Secure sign-in for every client, with short-lived org-bound tokens |
| Depends on | E01 |
| Features | Registration; password login; OAuth/OIDC server; sessions and refresh rotation; web auth; TOTP MFA and step-up; email codes; passkeys; DPoP and desktop login; API tokens |
| Tables | users, user\_mfa\_factors, webauthn\_credentials, mfa\_recovery\_codes, user\_sessions, refresh\_tokens, oauth\_clients, oauth\_authorizations, oauth\_authorization\_codes, one\_time\_tokens, auth\_events, personal\_access\_tokens, organization\_access\_tokens |
| APIs | /v1/auth/*, /oauth/*, /.well-known/\* |
| Frontend | Sign-up, sign-in, MFA enrolment, sessions page, security settings |
| Desktop | Loopback PKCE sign-in; key pair in keychain; DPoP proofs |
| Mobile | PKCE with app links; DPoP (V1) |
| Tests | Token rotation and reuse detection; brute-force limits; JWT key rotation; WebAuthn test vectors |
| Release | Alpha (core), Beta (MFA), V1 (passkeys, API tokens) |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E02-S01 | Registration and email verification | AC: no account enumeration; verification link single-use, 24 h | E01 | M | users, one\_time\_tokens | register, verify-email | API, Web |
| E02-S02 | Password login and lockout | Argon2id, breached-password check, 5-failure lockout, auth events. AC: timing-safe responses | S01 | M | users, auth\_events | login, password/\* | API, Web |
| E02-S03 | OAuth 2.1 / OIDC server | Code + PKCE (S256 only), ES256 JWT access tokens (15 min), JWKS, KMS-held keys, 90-day rotation. AC: tokens verify against JWKS; old `kid` accepted until expiry | S02 | XL | oauth\_clients, oauth\_authorization\_codes | /oauth/*, /.well-known/* | API |
| E02-S04 | Sessions and refresh rotation | Opaque refresh tokens, rotation on use, reuse detection revokes family, session list and revoke, Redis denylist. AC: replayed refresh token kills the session | S03 | L | user\_sessions, refresh\_tokens | refresh, sessions, logout | API, Web |
| E02-S05 | Web authentication flow | Access token in memory, refresh cookie (`HttpOnly`, `SameSite=Strict`) + CSRF, sign-in/up/forgot screens. AC: XSS cannot read the refresh token | S04 | M | — | auth/\* | Web |
| E02-S06 | TOTP MFA, recovery codes, step-up | AC: replayed TOTP step rejected; step-up valid 10 min; org enforcement hook for E04 | S04 | L | user\_mfa\_factors, mfa\_recovery\_codes | mfa/\*, step-up | API, Web |
| E02-S07 | Email one-time code | AC: 6 digits, 5 min, 10 per 30 min | S02 | S | one\_time\_tokens | email-code | API, Web |
| E02-S08 | Passkeys | WebAuthn registration and sign-in, sign-count checks. AC: works with platform authenticators on macOS, Windows, iOS, Android | S04 | L | webauthn\_credentials | passkeys/\* | API, Web |
| E02-S09 | Native sign-in with DPoP | Loopback-redirect PKCE for desktop, app-link PKCE for mobile, DPoP-bound refresh tokens. AC: a copied refresh token fails without the device key | S03 | L | refresh\_tokens | /oauth/token | API, Desktop, Mobile |
| E02-S10 | Personal and organization API tokens | Prefixed, hashed, scoped, expiring tokens. AC: prefix detectable by secret scanners | E04 | M | personal\_access\_tokens, organization\_access\_tokens | tokens | API, Web |

## E03 · Tenancy and organizations

| Field | Value |
| --- | --- |
| Purpose | The four isolation layers from Architecture §4, and the organization lifecycle |
| Depends on | E02 |
| Features | Organizations; tenant context and org-bound tokens; tenant models; RLS; composite keys; tenant-aware cache, queues, storage and channels; isolation test harness; Octane state guard; org settings |
| Tables | organizations (+ RLS helpers on every later table) |
| APIs | /v1/organizations\*, /v1/auth/switch-organization |
| Frontend | Create-org onboarding, org switcher, org settings |
| Desktop / Mobile | Org selection at sign-in when a user has several orgs |
| Tests | Cross-tenant replay suite; RLS fail-closed suite; Octane alternating-tenant test |
| Release | Alpha |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E03-S01 | Organizations and onboarding | Create org, owner membership, trial start, region pin. AC: new org has privacy-first defaults (E05-S05) | E02 | M | organizations | POST/GET organizations | API, Web |
| E03-S02 | Tenant context and org-bound tokens | `ResolveTenant`, immutable context, switch-organization. AC: no endpoint reads an org ID from input | S01 | L | user\_sessions | switch-organization, me | API, Web |
| E03-S03 | Tenant model base class | Global scope, creating hook, lint rule banning raw queries in domains. AC: architecture test fails for a tenant model not extending the base | S02 | M | — | — | API |
| E03-S04 | Row-level security | App DB role without BYPASSRLS; policy helper used by every migration; `SET LOCAL app.org_id` per request and job transaction. AC: queries without context return zero rows | S02 | XL | all tenant tables | — | API, Infra |
| E03-S05 | Composite foreign keys | Migration macro adding `UNIQUE (organization_id, id)` and composite FKs. AC: inserting a cross-tenant reference fails in PostgreSQL | S04 | M | all tenant tables | — | API |
| E03-S06 | Tenant-aware cache, queues, storage, channels | `TenantCache`, job middleware, S3 key builder, KMS encryption context, Reverb channel auth. AC: every path covered by a unit test with two orgs | S02 | L | — | broadcasting/auth | API |
| E03-S07 | Cross-tenant isolation harness | Generates a two-org fixture and replays every route with foreign IDs. AC: runs in CI on every PR; any 2xx fails the build | S04 | L | — | all | API |
| E03-S08 | Octane state guard | Per-request tenant binding, reset listeners, alternating-org stress test. AC: 10,000 alternating requests show zero bleed | S02 | M | — | — | API |
| E03-S09 | Organization settings | Profile, timezone, currency, week start, archive, transfer ownership. AC: timezone change recomputes rollups via invalidation | S01 | M | organizations | organizations/current\* | API, Web |

## E04 · Members, RBAC and teams

| Field | Value |
| --- | --- |
| Purpose | People in an organization and what each can see and do |
| Depends on | E03 |
| Features | Members and invitations; permission catalog and system roles; permission resolver; policies on every route; members UI; teams and leads; bulk import; job types and custom fields |
| Tables | organization\_members, invitations, permissions, roles, role\_permissions, role\_assignments, teams, team\_members, job\_types, custom\_fields, custom\_field\_values |
| APIs | /v1/users\*, /v1/users/invitations\*, /v1/invitations/accept, /v1/teams\*, /v1/job-types, /v1/custom-fields |
| Frontend | People list, invite dialog, member detail, teams pages |
| Desktop / Mobile | Show the member's role and projects |
| Tests | Permission matrix tests for every built-in role; relationship checks (team lead vs non-team member); seat-count tests |
| Release | Alpha (core), V1 (import) |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E04-S01 | Members and invitations | Single and bulk invites (≤ 500), accept flow for new and existing users, resend, revoke, emails. AC: accepting grants the roles, teams and projects in the invite | E03 | L | organization\_members, invitations | users, invitations, accept | API, Web |
| E04-S02 | Permission catalog and system roles | Seeded permissions and 8 built-in roles from Architecture §12. AC: role matrix test matches the spec table | S01 | M | permissions, roles, role\_permissions | GET roles | API |
| E04-S03 | Permission resolver | Union of org, team and project assignments with relationship checks; Redis cache keyed by permissions version. AC: p95 check < 1 ms from cache; invalidates on any assignment change | S02 | XL | role\_assignments | users/{id}/roles | API |
| E04-S04 | Policies everywhere | Policy per resource; CI check that every route calls `authorize`. AC: build fails for an unguarded route | S03 | M | — | all | API |
| E04-S05 | Members UI | List, filters, invite, edit, remove, reactivate, seat counter. AC: removing a member revokes sessions immediately | S01, S03 | M | organization\_members | users\* | Web |
| E04-S06 | Teams and team leads | Teams CRUD, membership, leads with per-permission toggles. AC: a lead sees only their team's data | S03 | M | teams, team\_members, role\_assignments | teams\* | API, Web |
| E04-S07 | Bulk member import | CSV/XLSX up to 5,000 rows as a job with per-row results. AC: partial failures reported, no partial rows | S01 | M | organization\_members, invitations | users/import | API, Web |
| E04-S08 | Job types and custom fields | AC: templates for common job types; custom fields on member profile | S01 | S | job\_types, custom\_fields, custom\_field\_values | job-types, custom-fields | API, Web |

## E05 · Policies, entitlements and privacy notice

| Field | Value |
| --- | --- |
| Purpose | The configuration every client obeys, gated by plan, and the notice that must precede tracking |
| Depends on | E04 |
| Features | Policy catalog and resolver; entitlements and usage counters; tracking settings UI; monitoring notice and acknowledgement; privacy-first defaults |
| Tables | policy\_definitions, policy\_values, plans, addons, prices, plan\_features, entitlements, usage\_counters, monitoring\_notices, consent\_records |
| APIs | /v1/organizations/current/policies\*, /v1/billing/entitlements, /v1/desktop/config (policy part), /v1/desktop/notice/acknowledge |
| Frontend | Settings → Tracking, Screenshots, Privacy; notice acknowledgement screen |
| Desktop | Applies config; blocks tracking until the notice is acknowledged |
| Mobile | Same notice gate (V1) |
| Tests | Resolution order tests (member > team > project > org > default); entitlement enforcement; notice version bump on policy change |
| Release | Alpha |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E05-S01 | Policy catalog and resolver | JSON-schema-validated values, scope resolution, per-org cache, `settings_version` bump. AC: effective value shows its source | E04 | L | policy\_definitions, policy\_values | policies\* | API |
| E05-S02 | Entitlements and usage counters | Plan catalog seeded; entitlements resolved per org; `EnforceEntitlement` middleware; Redis counters flushed to `usage_counters`. AC: a feature above the plan returns 403 `entitlement_required` | E03 | L | plans, addons, prices, plan\_features, entitlements, usage\_counters | billing/entitlements | API |
| E05-S03 | Tracking settings UI | Screenshots (frequency, blur, deletion), idle timeout, app/URL level, per-member and per-team overrides. AC: changes visible to agents within one config poll | S01 | M | policy\_values | policies\* | Web |
| E05-S04 | Monitoring notice and acknowledgement | Notice generated from policies; new version on relevant change; acknowledgement in web and agent. AC: agent refuses to track until the latest version is acknowledged | S01 | M | monitoring\_notices, consent\_records | notice/acknowledge | API, Web, Desktop |
| E05-S05 | Privacy-first defaults | New orgs: screenshots off, domain-only URLs, titles off, GPS off. AC: onboarding asks the admin to choose, and the notice reflects the choice | S01 | S | policy\_values | — | API, Web |

## E06 · Projects, clients, tasks, rates and budgets

| Field | Value |
| --- | --- |
| Purpose | What people track time against, and what it costs |
| Depends on | E04 (E05 for entitlements on budgets) |
| Features | Clients; projects and membership; tasks; pickers and pages; rates; budgets and alerts |
| Tables | clients, projects, project\_members, tasks, task\_assignees, rates, budgets, budget\_alert\_events |
| APIs | /v1/clients\*, /v1/projects\*, /v1/tasks\*, /v1/rates\*, /v1/budgets\* |
| Frontend | Projects list and detail, clients, tasks board/list, budget widget |
| Desktop | Project and task picker (via `/v1/desktop/projects` delta) |
| Mobile | Project and task picker (V1) |
| Tests | Rate overlap constraint; budget threshold crossing once per period; archived project stops timers |
| Release | Alpha (projects, tasks, clients), V1 (rates, budgets) |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E06-S01 | Clients | CRUD, archive, restore. AC: archived clients hidden from pickers but kept in reports | E04 | S | clients | clients\* | API, Web |
| E06-S02 | Projects and membership | CRUD, archive (stops running timers), membership by member or team with project roles. AC: only members can track on a project | S01 | M | projects, project\_members | projects\* | API, Web |
| E06-S03 | Tasks | CRUD, subtasks, assignees, complete/reopen. AC: `integration_managed` projects reject native edits (409) | S02 | M | tasks, task\_assignees | tasks\* | API, Web |
| E06-S04 | Project and task UI | List, detail, member management, task list with filters. AC: keyboard-friendly pickers reused by the web timer | S02, S03 | M | — | — | Web |
| E06-S05 | Rates with effective dates | Pay and bill rates per member (and per project), no-overlap constraint, field-level hiding. AC: amounts owed use the rate effective on the work date | S02 | M | rates | rates\* | API, Web |
| E06-S06 | Budgets and alerts | Hours or cost budgets, periods, thresholds, alert emails, optional timer stop. AC: each threshold fires once per period; stop applies to all devices within one sync | S05, E14 | L | budgets, budget\_alert\_events | budgets\* | API, Web |
| E06-S07 | Budget status and forecast | Consumed, remaining, linear forecast date. AC: matches the budget report to the minute | S06 | S | rollup\_member\_daily | budgets/{id}/status | API, Web |

## E07 · Devices and agent protocol

| Field | Value |
| --- | --- |
| Purpose | The server side of the agent: registration, config, sync ingest, heartbeats, commands and releases |
| Depends on | E05; E02-S09 (DPoP) |
| Features | Device registration; config and project delta; sync batch framework; heartbeats and presence; device commands; capabilities; devices UI; Ingest service; releases and updates |
| Tables | devices, device\_heartbeats, device\_commands, sync\_batches, agent\_releases |
| APIs | /v1/devices\*, /v1/desktop/\* |
| Frontend | Devices page; per-member capability indicators; who's online |
| Desktop | Consumes every endpoint here (E09) |
| Mobile | Uses register, config, sync, heartbeat (E24) |
| Tests | Batch idempotency (same batch twice → identical response); verdict framework; version gate (426); load test at 10,000 simulated devices |
| Release | Alpha |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E07-S01 | Device registration | Idempotent by `install_id`, public key stored, minimum-version gate. AC: re-install on the same machine is detected via hardware hash | E05 | M | devices | desktop/register | API |
| E07-S02 | Config and project delta | Policies + entitlements with ETag/304; project/task delta with deletions. AC: a policy change reaches the agent within one poll or push | S01 | M | — | desktop/config, desktop/projects | API |
| E07-S03 | Sync batch framework | Envelope validation, `sync_batches` receipt, ordered per-member application, handler registry per event type, per-event verdicts, cached response replay. AC: replaying a batch returns the identical response; handlers are added per epic | S01 | XL | sync\_batches | desktop/sync | API |
| E07-S04 | Heartbeats and presence | Redis presence, 5-minute sampler to `device_heartbeats`, who's-online feed. AC: offline shown within 3 minutes of last heartbeat | S01 | M | device\_heartbeats | desktop/heartbeat, timer/team | API, Web |
| E07-S05 | Device commands | Queue, delivery by heartbeat response and WebSocket, acknowledgement, expiry. AC: `stop_timer` delivered within 60 s to an online device | S04 | M | device\_commands | devices/{id}/commands, commands/{id}/ack | API |
| E07-S06 | Capability reporting | Store capability map; dashboard badges; alert admins on newly denied capability. AC: a member with denied screen recording shows a clear badge | S01 | S | devices | desktop/capabilities | API, Web |
| E07-S07 | Devices UI | List, rename, revoke, send command. AC: revoke signs the device out within one heartbeat | S05 | M | devices | devices\* | Web |
| E07-S08 | Ingest service deployment | Separate ECS service for `/v1/desktop/*`, autoscaling on requests and queue depth. AC: web API unaffected during an ingest load test | S03 | M | — | desktop/\* | Infra |
| E07-S09 | Releases and update feed | Release records, rollout percentage, signature, minimum supported version. AC: 10% rollout reaches about 10% of devices | S01 | M | agent\_releases | desktop/updates | API, Infra |

## E08 · Time tracking core

| Field | Value |
| --- | --- |
| Purpose | The source of truth for tracked time on every client |
| Depends on | E06, E07 |
| Features | Timer and time-entry model; start/stop/switch; manual time; manual-time approvals; idle; limits; web timer; time views; locking; conflict rules |
| Tables | timers, time\_entries, time\_entry\_edits, manual\_time\_requests, idle\_periods |
| APIs | /v1/timer\*, /v1/time-entries\*, /v1/manual-time-requests\*, /v1/idle-periods\*; sync handlers for timer and idle events |
| Frontend | Header timer, time views (day, week, calendar), edit dialogs, approvals queue |
| Desktop | Timer UI and idle dialog (E09) |
| Mobile | Timer (E24) |
| Tests | Overlap constraint; one-running-timer index; property tests for offline sequences; lock rules per role |
| Release | Alpha (core), Beta (manual approvals), V1 (limits) |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E08-S01 | Timer and time-entry schema | No-overlap exclusion constraint, one-running-timer index, edit history. AC: the database rejects overlapping entries for a member | E06 | L | timers, time\_entries, time\_entry\_edits | — | API |
| E08-S02 | Start, stop, switch and takeover | One service used by the API and sync handlers; takeover stops the other device via command. AC: two simultaneous starts → one timer; the other device is told to stop | S01, E07-S05 | XL | timers, time\_entries | timer/\* ; sync `timer.*` | API |
| E08-S03 | Manual time | Add, edit, delete, split with required reasons and history; overlap handling. AC: deleting time also deletes its activity, usage and screenshots | S01 | L | time\_entries, time\_entry\_edits | time-entries\* | API, Web |
| E08-S04 | Manual-time approvals | Policy switch; requests, approve/deny (bulk). AC: approved request creates exactly one entry | S03 | M | manual\_time\_requests | manual-time-requests\* | API, Web |
| E08-S05 | Idle periods | Ingest from agent, resolve endpoint, keep-idle policy (prompt/always/never). AC: discard removes the idle interval from the entry and its slots | S02 | M | idle\_periods | idle-periods\*, sync `idle.*` | API, Web |
| E08-S06 | Time limits | Daily/weekly caps, blocked days, shift-only (with E25); enforced at start and ingest. AC: agent stops within one sync of reaching the cap | S02 | M | policy\_values | timer/start; sync | API |
| E08-S07 | Web timer | Header timer, project/task picker, today total, break button (with E26). AC: timer state stays in sync with the desktop agent via WebSocket | S02 | M | — | timer/\* | Web |
| E08-S08 | Time views | Day, week and calendar views of own and others' time with edit dialogs, timezone switch. AC: totals match rollups exactly | S03 | L | — | time-entries\* | Web |
| E08-S09 | Locked time | Lock/unlock rules by role (timesheet, payment, invoice, export, integration). AC: edits to locked time return 423 | S03 | M | time\_entries | time-entries\* | API |
| E08-S10 | Conflict rules spec and property tests | Written spec of offline, overlap, lock and multi-device rules; property-based test generator shared with E09. AC: 100,000 random sequences never produce overlap or lost time | S02 | L | — | sync | API, Desktop |
| E08-S11 | Automatic start/stop by schedule | Uses shifts (E25) or fixed hours; optional wait for first input. AC: agent starts within 60 s of shift start when running | E25 | M | policy\_values | desktop/config | API, Desktop |

## E09 · Desktop agent core

| Field | Value |
| --- | --- |
| Purpose | The shared Rust engine and React UI of the Tauri agent (Architecture §5, §13, §14) |
| Depends on | E00-S09; E07 (protocol); E08-S10 (conflict spec). OS spikes (E10–E13 S01) start in stage 1 |
| Features | Crates and IPC; encrypted storage; native sign-in; tracker and slots; sync engine; config and notice gate; screenshot pipeline; idle and power; UI; updater; watchdog and diagnostics; usage aggregation; input features; simulation harness |
| Tables | Local SQLCipher schema (journal, outbox, slots, spool index, config cache) |
| APIs | All of /v1/desktop/\*, /oauth/token |
| Frontend | Agent UI in the webview |
| Tests | Simulation harness with fake platform adapters and a controllable clock; property tests from E08-S10; crash-injection tests |
| Release | Alpha |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E09-S01 | Crates, IPC and shell | `core`, `sync`, `storage`, `platform` crates; typed Tauri commands with generated TS types; strict CSP; single instance; tray-resident. AC: UI cannot call anything not on the allowlist | E00-S09 | M | local | — | Desktop |
| E09-S02 | Encrypted local storage | SQLCipher with key in OS keychain, WAL, migrations, journal, outbox with `seq`, encrypted spool, size caps. AC: pulling the power mid-write loses nothing that was journaled | S01 | L | local | — | Desktop |
| E09-S03 | Native sign-in | Loopback PKCE, device key pair, DPoP proofs, token refresh, org choice. AC: sign-in works behind a corporate proxy | S01, E02-S09 | L | local | /oauth/\*, desktop/register | Desktop |
| E09-S04 | Tracker and slot aggregator | State machine (stopped, running, idle-pending, paused, on-break), 10-minute slots, monotonic durations. AC: slot totals equal wall time within 1 s over 24 h of simulation | S02 | XL | local | — | Desktop |
| E09-S05 | Sync engine | Ordered batches ≤ 500 events, backoff with jitter, verdict handling (accepted, duplicate, conflict, rejected), clock offset, re-login flow, command handling. AC: 7 days offline syncs completely with no duplicates | S02, S03, E07-S03 | XL | local | desktop/sync, heartbeat, commands | Desktop |
| E09-S06 | Config, notice gate and capabilities | Applies policy config, polls or receives pushes, blocks tracking until notice acknowledged, reports capability map. AC: config change applied without restart | S05, E05-S04 | M | local | desktop/config, notice, capabilities | Desktop |
| E09-S07 | Screenshot pipeline | Random time per slot, per-policy count, blur, WebP, encrypted spool, presigned upload, completion event. AC: no capture while idle, paused or on break; spool emptied only after server confirmation | S04, S05, E15-S01 | XL | local | screenshots/upload-urls | Desktop |
| E09-S08 | Idle and power handling | Idle detection via platform trait, idle dialog (keep, discard, reassign, stop), sleep/wake closes slots. AC: 2-hour sleep produces no tracked time and one idle prompt at most | S04 | M | local | sync `idle.*` | Desktop |
| E09-S09 | Agent UI | Timer, project/task picker, today total, idle dialog, break button, permission wizard, "my data" (own screenshots and activity), settings, offline indicator. AC: all screens keyboard accessible | S04 | L | — | — | Desktop |
| E09-S10 | Auto-update | Tauri updater with signed packages, stable/beta channels, rollout gating, forced update below minimum. AC: a bad signature is rejected; rollback by publishing the previous version | S01, E07-S09 | M | — | desktop/updates | Desktop, Infra |
| E09-S11 | Watchdog and diagnostics | Restart within 5 s after crash, native crash reports (Sentry), log bundle upload on command. AC: crash-loop protection after 5 restarts in 5 min | S01 | M | — | logs/upload-url | Desktop |
| E09-S12 | App/URL usage aggregation | Per-slot aggregation of foreground app and domain time from platform events, privacy levels (off, apps, domains, paths). AC: private windows follow the policy | S04 | M | local | sync `app_usage.batch` | Desktop |
| E09-S13 | Input timing features | Per-slot statistics (interval entropy, keyboard/mouse ratio) computed on device for anomaly detection. AC: no raw events leave the device | S04 | M | local | sync | Desktop |
| E09-S14 | Simulation harness | Fake adapters, controllable clock, scripted scenarios (offline, sleep, clock change, crash). AC: runs in CI on Linux for every PR | S04 | L | — | — | Desktop |

## E10 · Desktop platform: macOS

| Field | Value |
| --- | --- |
| Purpose | macOS implementations of the platform traits, permissions, packaging and signing |
| Depends on | E09-S01 (traits); spike S01 in stage 1 |
| Features | Screen capture; input and idle; active app; browser URLs; permission health; menu bar and login item; packaging and notarization; MDM package |
| Tables | — (local agent storage from E09) |
| APIs | Uses /v1/desktop/\* via E09; /v1/desktop/capabilities |
| Frontend | Permission wizard screens in the agent UI (macOS variants) |
| Desktop | All stories below |
| Mobile | — |
| Tests | Automated + manual matrix on macOS 13–15 and current, Apple Silicon and Intel; TCC denial and revocation scenarios |
| Release | Alpha |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E10-S01 | Spike: capture, input, permissions | Prove ScreenCaptureKit, event tap and URL reading in a throwaway app; document every prompt. AC: written findings and go/no-go | — | M | — | — | Desktop |
| E10-S02 | Screen capture | ScreenCaptureKit, all or active monitor, Retina downscale. AC: capture < 300 ms and < 5% CPU spike | S01 | L | — | — | Desktop |
| E10-S03 | Input activity and idle | CGEvent tap (Input Monitoring) for per-second activity; seconds-since-last-input for idle. AC: keyboard and mouse separated | S01 | L | — | — | Desktop |
| E10-S04 | Active app and window | NSWorkspace front app; title via Accessibility when enabled. AC: bundle IDs normalized | S01 | M | — | — | Desktop |
| E10-S05 | Browser URLs | AppleScript for Safari, Chrome, Edge, Brave; Accessibility fallback for Firefox. AC: domain-only mode never reads full paths into memory beyond parsing | S04 | L | — | — | Desktop |
| E10-S06 | Permission health and wizard | Detect Screen Recording, Accessibility, Input Monitoring, Automation state; deep links to System Settings; re-check on wake. AC: revoked permission reported within 1 minute | S02–S05 | M | — | desktop/capabilities | Desktop |
| E10-S07 | Menu bar, login item, power, network | NSStatusItem, SMAppService login item, sleep/wake, network identity hash. AC: starts at login without a visible window | E09-S01 | M | — | — | Desktop |
| E10-S08 | Packaging, signing, notarization | Universal binary, hardened runtime, entitlements, Developer ID signing, notarization and stapling in CI. AC: Gatekeeper opens it without warnings | E09-S10 | M | — | — | Desktop, Infra |
| E10-S09 | MDM silent package | .pkg + PPPC profile for Jamf/Intune; silent mode flag. AC: installs with no user prompts except Screen Recording | E30 | M | — | — | Desktop |

## E11 · Desktop platform: Windows

| Field | Value |
| --- | --- |
| Purpose | Windows implementations, installer, signing and update service |
| Depends on | E09-S01; spike S01 in stage 1 |
| Features | Screen capture; input and idle; active app and URLs; tray and auto-start; installer and signing; update service; Intune deployment |
| Tables | — |
| APIs | Uses /v1/desktop/\* via E09 |
| Frontend | Windows-specific settings (start with Windows) in the agent UI |
| Desktop | All stories below |
| Mobile | — |
| Tests | Windows 10 22H2 and 11 (x64); mixed DPI; standard (non-admin) user; antivirus false-positive checks |
| Release | Alpha |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E11-S01 | Spike: capture, input, URL | Throwaway proof of Graphics Capture, raw input and UI Automation URL reading. AC: go/no-go notes | — | M | — | — | Desktop |
| E11-S02 | Screen capture | Windows.Graphics.Capture with Desktop Duplication fallback; per-monitor DPI. AC: correct on mixed-DPI monitors | S01 | L | — | — | Desktop |
| E11-S03 | Input activity and idle | Raw Input for per-second activity; `GetLastInputInfo` for idle. AC: works for standard users | S01 | M | — | — | Desktop |
| E11-S04 | Active app and URLs | Foreground window and process; UI Automation address bar for Chrome, Edge, Firefox, Brave. AC: exe names normalized | S01 | L | — | — | Desktop |
| E11-S05 | Tray, auto-start, power, network | Notification-area icon, Run-key auto-start, power broadcast events, network identity. AC: survives Explorer restart | E09-S01 | M | — | — | Desktop |
| E11-S06 | Installer and signing | MSI (WiX) and EXE (NSIS), per-user install, Authenticode with EV certificate in a cloud HSM. AC: no SmartScreen warning after reputation builds | E09-S10 | M | — | — | Desktop, Infra |
| E11-S07 | Update service and watchdog | Optional Windows service that applies updates and restarts the per-user agent. AC: tracker never runs in session 0 | S06 | M | — | — | Desktop |
| E11-S08 | Intune/GPO silent deployment | Silent MSI parameters, org token enrolment. AC: 100 test machines enrol unattended | E30 | M | — | — | Desktop |

## E12 · Desktop platform: Ubuntu X11

| Field | Value |
| --- | --- |
| Purpose | Full-feature Linux support on X11 sessions |
| Depends on | E09-S01; spike S01 in stage 1 |
| Features | Screen capture; input and idle; active window and URLs; tray and autostart; packages and repository |
| Tables | — |
| APIs | Uses /v1/desktop/\* via E09 |
| Frontend | Linux notes in the agent's permission wizard |
| Desktop | All stories below |
| Mobile | — |
| Tests | Ubuntu 22.04 and 24.04 LTS X11 sessions, GNOME and KDE; CI on Xvfb |
| Release | Private Beta |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E12-S01 | Spike: X11 APIs | XShm, XInput2, EWMH, AT-SPI proof. AC: go/no-go notes | — | S | — | — | Desktop |
| E12-S02 | Screen capture | XShm/XGetImage per XRandR monitor. AC: correct with 2 monitors of different sizes | S01 | M | — | — | Desktop |
| E12-S03 | Input and idle | XInput2 raw events; XScreenSaver idle. AC: combined keyboard+mouse figure where split unavailable | S01 | M | — | — | Desktop |
| E12-S04 | Active window and URLs | EWMH active window + WM\_CLASS; URLs via AT-SPI or the browser extension (E35). AC: falls back to extension when AT-SPI is off | S01 | M | — | — | Desktop |
| E12-S05 | Tray and autostart | StatusNotifierItem/AppIndicator, XDG autostart entry. AC: tray visible on stock Ubuntu | E09-S01 | S | — | — | Desktop |
| E12-S06 | Packaging and repository | .deb and .rpm, signed apt repository, updater via repo or in-app. AC: `apt upgrade` delivers new versions | E09-S10 | M | — | — | Desktop, Infra |

## E13 · Desktop platform: Ubuntu Wayland

| Field | Value |
| --- | --- |
| Purpose | Honest, reduced-capability support on Wayland (GNOME first) |
| Depends on | E12 (shared Linux code); E35 (URLs) |
| Features | Session detection and capability report; portal screenshots; idle and activity; GNOME active-window extension; KDE KWin script |
| Tables | devices.capabilities |
| APIs | /v1/desktop/capabilities |
| Frontend | "Reduced on Wayland" badges and explanations in web admin views |
| Desktop | All stories below |
| Mobile | — |
| Tests | Ubuntu 24.04 GNOME Wayland, Fedora GNOME; Kubuntu Plasma 6 Wayland (Future) |
| Release | V1 (GNOME, reduced), Future (KDE) |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E13-S01 | Spike: Wayland feasibility | Portal screenshot flow, idle-notify, GNOME extension prototype. AC: capability matrix confirmed per compositor | — | M | — | — | Desktop |
| E13-S02 | Session detection and capability report | Detect Wayland and compositor; report unsupported features to the server; UI explains limits. AC: admins see "Wayland: reduced" per device | E12 | S | devices | desktop/capabilities | Desktop, Web |
| E13-S03 | Portal screenshots | xdg-desktop-portal Screenshot with persisted permission where supported. AC: one consent, then unattended capture on GNOME 46+ or documented otherwise | S01 | XL | — | — | Desktop |
| E13-S04 | Idle and activity | ext-idle-notify-v1 or Mutter IdleMonitor; per-second activity approximated from idle transitions. AC: activity % within 5 points of X11 on the same workload | S01 | L | — | — | Desktop |
| E13-S05 | GNOME active-window extension | Small GNOME Shell extension exposing the focused app over D-Bus; published on extensions.gnome.org. AC: works on the GNOME version shipped with Ubuntu 24.04 LTS | S01 | L | — | — | Desktop |
| E13-S06 | KDE KWin script | KWin script exposing the active window. AC: works on Plasma 6 | S05 | M | — | — | Desktop |

## E14 · Activity ingestion and rollups

| Field | Value |
| --- | --- |
| Purpose | Store 10-minute activity slots and keep reporting rollups current |
| Depends on | E07-S03, E08 |
| Features | Partitioned slots; slot ingest; rollups and reconciliation; activity API; activity UI; telemetry retention framework |
| Tables | activity\_slots, rollup\_member\_daily, rollup\_invalidations, retention\_runs |
| APIs | sync `slot.closed`; /v1/activity/\* |
| Frontend | Member day timeline, activity bars, team activity summary |
| Desktop | Sends slots (E09-S04) |
| Mobile | — (mobile time has no activity) |
| Tests | Rollup equals base data (property test); partition creation alarms; retention deletes only expired rows |
| Release | Alpha |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E14-S01 | Partitioned slot storage | pg\_partman monthly partitions, 3 months ahead, default partition, alarm below 2 future partitions. AC: inserts never fail for lack of a partition | E07-S03 | M | activity\_slots | — | API, Infra |
| E14-S02 | Slot ingest handler | Validation (aligned start, tracked ≤ 600, active ≤ tracked, entry and device consistency), clock flags, idempotent insert. AC: duplicate slot → `duplicate`; slot on locked time → `rejected: time_locked` | S01 | L | activity\_slots | sync | API |
| E14-S03 | Rollups and reconciliation | Invalidation queue, worker recomputing (member, day) within 60 s, nightly reconciliation job. AC: dashboard totals equal a direct base-table sum in 100% of reconciliation runs | S02 | XL | rollup\_member\_daily, rollup\_invalidations | — | API |
| E14-S04 | Activity API | Slots (≤ 31 days), daily (from rollups), team summary; aggregate-only mode respected. AC: scope rules from §12 | S03 | M | — | activity/\* | API |
| E14-S05 | Activity UI | Day timeline with slots, activity %, idle and manual segments; team summary. AC: renders a member-month in < 1 s | S04 | M | — | — | Web |
| E14-S06 | Telemetry retention framework | Per-org cutoffs per data class, batched deletes, partition drops beyond plan maximum, legal-hold hook, `retention_runs` log. AC: runs idempotently and resumably | S01 | L | retention\_runs | — | API |

## E15 · Screenshots

| Field | Value |
| --- | --- |
| Purpose | Presigned capture-to-dashboard pipeline with retention and access control (Architecture §14) |
| Depends on | E14, E05 (policies, quotas), E09-S07 (agent side) |
| Features | Upload URLs; processing; viewer; deletion; retention; my-data; bulk export |
| Tables | screenshots, screenshot\_access\_log |
| APIs | /v1/desktop/screenshots/upload-urls; /v1/screenshots\*; S3 → SQS events |
| Frontend | Screenshot grid by slot, lightbox, delete dialog |
| Desktop | Capture pipeline (E09-S07, E10–E13) |
| Mobile | — |
| Tests | Presigned URL misuse (wrong size, checksum, expired); access-scope tests; deletion removes the S3 object; retention and lifecycle backstop |
| Release | Alpha (core), V1 (bulk export) |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E15-S01 | Upload URL endpoint | Checks device, member, slot count, quota; creates rows; presigned PUT with exact length, SHA-256 checksum, SSE-KMS and retention tag. AC: a mismatched upload is rejected by S3 | E14-S02 | L | screenshots | screenshots/upload-urls | API, Infra |
| E15-S02 | Processing worker | S3 event → SQS → verify, decode, strip metadata, thumbnail, `ready`, broadcast. AC: 95% visible within 2 minutes; failures to DLQ with alert | S01 | L | screenshots | — | API, Infra |
| E15-S03 | Viewer and API | Grid by slot, lightbox, presigned GET (60 s), batched access log. AC: a user outside scope gets 404; every view is logged | S02 | L | screenshots, screenshot\_access\_log | screenshots\* | API, Web |
| E15-S04 | Deletion | keep\_time (admins) and remove\_time (own block, if allowed); S3 delete; audit. AC: object gone from S3 within 1 minute | S03 | M | screenshots, time\_entries | DELETE screenshots/{id} | API, Web |
| E15-S05 | Retention and bucket policy | Nightly deletes + tag-based lifecycle backstop; Terraform bucket policy (TLS, KMS, no public). AC: no object older than its class maximum + 7 days exists | S01, E14-S06 | M | screenshots, retention\_runs | — | API, Infra |
| E15-S06 | Own-data access | Members see their own screenshots in web and agent. AC: works even when managers lack view permission | S03 | S | — | screenshots\* | Web, Desktop |
| E15-S07 | Bulk export | Owner-only ZIP export with step-up, 7-day link. AC: export audited | S03 | M | report\_exports | screenshots/exports | API, Web |

## E16 · Applications and websites

| Field | Value |
| --- | --- |
| Purpose | App and URL usage with privacy levels and productivity classification |
| Depends on | E14 |
| Features | Global catalogs; usage ingest; usage rollups and API; UI; classification |
| Tables | applications, web\_domains, app\_usage, url\_usage, rollup\_usage\_daily, productivity\_classifications |
| APIs | sync `app_usage.batch`, `url_usage.batch`; /v1/applications\*, /v1/websites\*, /v1/classifications\* |
| Frontend | Apps & URLs pages; classification settings |
| Desktop | E09-S12 + OS adapters |
| Mobile | — |
| Tests | Privacy level enforced server-side (paths dropped when domain-only); normalization of app identifiers across OSes |
| Release | Alpha (apps, domains), V1 (classification) |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E16-S01 | Global catalogs | Normalize app identifiers per OS and domains to eTLD+1; icons; default categories. AC: "Google Chrome" is one entry per platform | E14 | M | applications, web\_domains | — | API |
| E16-S02 | Usage ingest | Partitioned usage tables; server re-applies the privacy level (drops paths and titles the policy forbids). AC: a misconfigured agent cannot store forbidden detail | S01 | L | app\_usage, url\_usage | sync | API |
| E16-S03 | Usage rollups and API | Daily rollups; usage endpoints grouped by app, member, day, project. AC: 92-day range returns in < 2 s | S02, E14-S03 | M | rollup\_usage\_daily | applications/usage, websites/usage | API |
| E16-S04 | Apps & URLs UI | Per member and team; top apps and domains; time of day. AC: aggregate-only mode hides member-level views | S03 | M | — | — | Web |
| E16-S05 | Productivity classification | Org rules for apps, domains and categories; templates; job-type and team scopes; re-queue rollups on change. AC: a rule change updates reports within 10 minutes | S03, E04-S08 | M | productivity\_classifications | classifications\* | API, Web |

## E17 · Web app and dashboards

| Field | Value |
| --- | --- |
| Purpose | The manager and member experience that ties every domain together |
| Depends on | E04, E08, E14 |
| Features | Navigation and org switcher; manager dashboard; member home; realtime; onboarding; accessibility; marketing site |
| Tables | — (reads other domains) |
| APIs | /v1/reports/dashboard, /v1/timer/team, broadcasting auth |
| Frontend | Everything listed |
| Desktop / Mobile | Download page and deep links |
| Tests | Playwright E2E for owner, manager and member journeys; axe accessibility checks |
| Release | Alpha (shell, dashboard), Beta (onboarding), Public Beta (marketing site) |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E17-S01 | Navigation and org switcher | Role-aware menus, org switcher, profile menu. AC: hidden menus match permissions exactly | E04 | M | — | auth/me | Web |
| E17-S02 | Manager dashboard | Who's online, today's hours, activity, recent screenshots, top projects, alerts. AC: loads in < 1 s for a 200-member org | E14-S03 | L | — | reports/dashboard | API, Web |
| E17-S03 | Member home | My time today and this week, my activity, notice status, my data. AC: members never see others' data here | E14-S04 | M | — | — | Web |
| E17-S04 | Realtime client | Reverb connection, channel subscriptions, reconnect. AC: timer changes appear in < 2 s | E03-S06 | M | — | broadcasting/auth | Web |
| E17-S05 | Onboarding checklist | Invite members, choose policies, download agent, first tracked hour. AC: checklist completion tracked for activation metrics | S02 | M | — | — | Web |
| E17-S06 | Accessibility and localisation pass | WCAG 2.2 AA, keyboard navigation, i18n strings. AC: zero critical axe violations | S02 | M | — | — | Web |
| E17-S07 | Marketing site | Next.js static: home, pricing, downloads, security, privacy, docs entry. AC: Lighthouse ≥ 90 | E00 | M | — | billing/plans | Web |

## E18 · Reports and exports

| Field | Value |
| --- | --- |
| Purpose | One report engine serving UI, exports, schedules, API and AI (Architecture §15) |
| Depends on | E14, E16 |
| Features | Report engine; core reports; exports; report UI; saved and scheduled reports; replica routing. Domain reports are added by their epics |
| Tables | saved\_reports, report\_schedules, report\_exports |
| APIs | /v1/reports/\*, /v1/jobs/{id} |
| Frontend | Report builder, exports list, schedules |
| Desktop / Mobile | — |
| Tests | Golden-file tests per report; totals equal time views; timezone edge cases (DST, day boundaries) |
| Release | Alpha (engine, core reports), V1 (saved and scheduled) |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E18-S01 | Report engine | Typed query classes, declared filters/columns/permissions, data scope, timezone, totals, cursor paging. AC: one class feeds UI, export and API identically | E14-S03 | XL | — | reports/types, reports/query | API |
| E18-S02 | Core reports | Time & Activity, Daily Totals, Work Sessions, Manual Time Edits, Apps & URLs. AC: golden files pass across DST changes | S01 | L | — | reports/query | API |
| E18-S03 | Exports | Async CSV and XLSX (streamed) and PDF (Gotenberg), S3 storage, 7-day links, 10/hour limit. AC: 1 M-row CSV completes without memory growth | S01 | L | report\_exports | reports/exports\* | API, Infra |
| E18-S04 | Report builder UI | Filters, columns, group-by, totals, export button. AC: URL encodes the full report state | S02 | L | — | — | Web |
| E18-S05 | Saved and scheduled reports | Save, favourite, share org-wide; daily/weekly/monthly email delivery. AC: schedule runs in the org timezone | S03 | M | saved\_reports, report\_schedules | reports/saved\*, reports/schedules\* | API, Web |
| E18-S06 | Replica routing and budgets | Read-replica connection for reports; p95 budget alerts per report type. AC: primary CPU unaffected by a report load test | S01 | M | — | — | API, Infra |

## E19 · Timesheets and approvals

| Field | Value |
| --- | --- |
| Purpose | Pay-period timesheets that lock time and feed payroll and invoices |
| Depends on | E08 |
| Features | Generation; submit and lock; approvals (single, bulk, multi-step); approvals UI; approval report |
| Tables | timesheets, timesheet\_approvals |
| APIs | /v1/timesheets\* |
| Frontend | My timesheets; approvals queue |
| Desktop | Blocks tracking into a submitted period; shows status |
| Mobile | View and submit (V1) |
| Tests | State machine tests; cannot self-approve; locked-time edits return 423 |
| Release | MVP (view, submit), V1 (approvals) |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E19-S01 | Timesheet generation | Per member per pay period (policy), totals snapshot. AC: created lazily on first tracked minute of a period | E08 | M | timesheets | timesheets | API |
| E19-S02 | Submit and lock | Submit, unsubmit, lock the period for new tracking. AC: agent receives `rejected: time_locked` for late events in the period | S01 | M | timesheets, time\_entries | submit, unsubmit | API, Web, Desktop |
| E19-S03 | Approvals | Approve, deny, reopen, bulk, multi-step chains, no self-approval. AC: approval locks all entries in the period | S02 | L | timesheet\_approvals | approve, deny, reopen, bulk-approve | API |
| E19-S04 | Approvals UI | Queue with hours, manual time, activity, overtime, PTO and amount owed per member. AC: bulk approve 100 timesheets in one action | S03 | M | — | timesheets\* | Web |
| E19-S05 | Timesheet approval report | AC: CSV export | S03, E18 | S | — | reports/query | API |

## E20 · Billing and subscriptions

| Field | Value |
| --- | --- |
| Purpose | Charge customers per seat with plans, add-ons, trials and dunning (Architecture §16) |
| Depends on | E05-S02 (entitlements), E04 (seats) |
| Features | Provider decision and interface; checkout; seats; plan changes; trial and suspension; billing UI; add-ons; usage quotas |
| Tables | billing\_customers, subscriptions, subscription\_items, seat\_events, billing\_invoices, billing\_webhook\_events (+ E05 catalog tables) |
| APIs | /v1/billing/\*, /hooks/billing/{provider} |
| Frontend | Billing page, plan picker, upgrade prompts, suspended-state banner |
| Desktop | Stops tracking when the org is suspended (config) |
| Mobile | Same |
| Tests | Provider sandbox end-to-end; webhook replay idempotency; proration arithmetic; suspension blocks writes but not billing endpoints |
| Release | Private Beta (test mode), Public Beta (live) |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E20-S01 | Provider decision and interface | Confirm entity and provider eligibility (ADR-007); `BillingProvider` interface; sandbox account. AC: ADR accepted | — | M | — | — | API |
| E20-S02 | Checkout and webhooks | Checkout session, customer record, subscription creation, idempotent webhook processing. AC: replaying every webhook twice changes nothing | S01 | L | billing\_customers, subscriptions, subscription\_items, billing\_webhook\_events | checkout, /hooks/billing | API, Web |
| E20-S03 | Seats and proration | Seat events on member changes, sync to provider, published proration rules. AC: adding 3 seats mid-term charges the prorated amount shown in preview | S02 | L | seat\_events | subscription | API |
| E20-S04 | Plan changes | Preview, upgrade now, downgrade at period end, conflict detection (features in use). AC: downgrade blocked with a list of conflicts until resolved | S02 | L | subscriptions | preview, change | API, Web |
| E20-S05 | Trial, dunning, suspension | 14-day trial; past-due grace 7 days; suspended = read-only, agents stop, billing still reachable. AC: paying restores access within 1 minute | S02 | M | organizations, subscriptions | subscription, cancel, resume | API, Web, Desktop |
| E20-S06 | Billing UI | Current plan, seats, usage, invoices, customer and tax details, provider portal. AC: owner and billing admin only | S02 | M | billing\_invoices | billing/\* | Web |
| E20-S07 | Add-ons | Insights, Locations, Silent agent, More screenshots, Retention, AI add-ons as entitlements. AC: enabling an add-on unlocks the feature without re-login | S03 | M | subscription\_items, entitlements | subscription/change | API, Web |
| E20-S08 | Usage quotas | Screenshot and app/URL caps on lower plans via Redis counters. AC: at cap, upload URLs return `quota_exceeded` and the agent pauses capture | S02, E15-S01 | M | usage\_counters | upload-urls, billing/usage | API, Desktop |

## E21 · Notifications and alerts

| Field | Value |
| --- | --- |
| Purpose | Tell the right person at the right time, in-app, by email and by push |
| Depends on | E01 (outbox), E04 |
| Features | Notification core and email; push; alert rules; digests |
| Tables | notification\_types, notification\_preferences, notifications, notification\_deliveries, push\_tokens, alert\_rules |
| APIs | /v1/notifications\* (inbox, preferences), alert-rule CRUD under /v1/alert-rules |
| Frontend | Bell inbox, preferences page, alert rules settings |
| Desktop | Native notifications for reminders and idle |
| Mobile | Push (V1) |
| Tests | Dedupe per event; preference respected; mandatory security notices always sent |
| Release | Beta (core, email), V1 (push, alert rules) |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E21-S01 | Notification core and email | Types, preferences, inbox, deliveries, SES templates, bounce handling. AC: one notification per event per member | E01-S02 | L | notification\_types, notification\_preferences, notifications, notification\_deliveries | notifications\* | API, Web |
| E21-S02 | Push notifications | APNs and FCM, token lifecycle, invalid-token cleanup. AC: delivered within 30 s | S01, E24 | M | push\_tokens | — | API, Mobile |
| E21-S03 | Alert rules | Low activity, high idle, device offline, budget, overtime, shift events; scoped recipients. AC: each rule fires once per window | S01, E14 | L | alert\_rules | alert-rules\* | API, Web |
| E21-S04 | Digests | Daily/weekly email summaries for managers and members. AC: sent in recipient's timezone at 08:00 | S01, E18 | M | — | — | API |

## E22 · Public API, webhooks and developer platform

| Field | Value |
| --- | --- |
| Purpose | Let customers and partners build on Worklume |
| Depends on | E02-S03, E02-S10, E18 |
| Features | Developer docs and SDK; third-party OAuth apps; customer webhooks; event catalog; API tiers; Zapier; MCP server |
| Tables | oauth\_clients, oauth\_authorizations, webhook\_endpoints, webhook\_deliveries |
| APIs | /v1/webhooks\*, /oauth/\* (third-party), all public endpoints |
| Frontend | Developer settings: tokens, OAuth apps, webhooks, delivery log |
| Desktop / Mobile | — |
| Tests | Signature verification sample code tested; SSRF tests; retry schedule; scope ∩ permission tests |
| Release | V1 (API, webhooks, Zapier), V3/Future (MCP) |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E22-S01 | Developer docs and SDK | Published reference from OpenAPI, guides, TypeScript SDK. AC: every endpoint has an example | E00-S07 | M | — | — | Web |
| E22-S02 | Third-party OAuth apps | Org-owned OAuth clients, consent screen, scopes, revocation. AC: effective access = scope ∩ granting member's permissions | E02-S03 | L | oauth\_clients, oauth\_authorizations | /oauth/\*, apps | API, Web |
| E22-S03 | Customer webhooks | Endpoints, verification ping, HMAC signatures, delivery worker (8 retries/24 h), auto-disable, replay, SSRF guard. AC: private-IP URLs rejected | E01-S02 | L | webhook\_endpoints, webhook\_deliveries | webhooks\* | API, Web |
| E22-S04 | Event catalog | Versioned payload schemas for all v1 events. AC: schema tests per event | S03 | M | — | webhooks/event-types | API |
| E22-S05 | API usage and tiers | Per-client usage stats and limit tiers. AC: org can see calls per token | E01-S08 | S | — | — | API, Web |
| E22-S06 | Zapier app | Triggers via REST hooks, actions (create task, add time, create client). AC: approved in the Zapier directory | S02, S03 | M | — | — | API |
| E22-S07 | MCP server | Read-only MCP over the public API with OAuth. AC: honours scopes and data scope | S02 | M | — | — | API |

## E23 · Integrations

| Field | Value |
| --- | --- |
| Purpose | Connect Worklume to project, accounting, HR, calendar and chat tools (Architecture §17) |
| Depends on | E01-S02, E06, E08; E22-S03 for shared signing code |
| Features | Connector framework; inbound events and polling; write-back; Jira; Asana and ClickUp; GitHub and GitLab; Slack and Teams; integrations UI; QuickBooks and Xero; BambooHR; calendars |
| Tables | integration\_providers, integration\_connections, integration\_project\_links, integration\_user\_links, integration\_entity\_maps, integration\_sync\_runs, integration\_inbound\_events |
| APIs | /v1/integrations\*, /hooks/integrations/{provider} |
| Frontend | Integrations catalog, connect wizard, link mapping, sync log |
| Desktop | Imported tasks appear in the picker |
| Mobile | Same |
| Tests | Recorded-fixture contract tests per provider; loop prevention; token refresh; circuit breaker |
| Release | V1 (framework + 6 connectors), V2 (accounting, HR, calendar) |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E23-S01 | Connector framework | Contract, KMS-encrypted credentials, OAuth connect with signed state, entity maps, sync runs, per-connection rate limit, circuit breaker. AC: a new connector needs only the contract implementation | E01 | XL | integration\_\* | integrations/\* | API |
| E23-S02 | Inbound events and polling | Provider webhook receiver with signature checks; incremental polling scheduler. AC: tasks appear within 5 minutes without webhooks, 30 s with | S01 | L | integration\_inbound\_events | /hooks/integrations | API |
| E23-S03 | Write-back engine | Modes hourly/daily/delayed/on-complete/off from `time_entry.*` events; loop prevention by content hash. AC: an edit made in the tool is not echoed back | S01 | L | integration\_entity\_maps | — | API |
| E23-S04 | Jira Cloud | OAuth 2.0 (3LO); projects, issues, epics, worklogs. AC: worklogs match Worklume time to the minute | S02, S03 | L | — | — | API |
| E23-S05 | Asana and ClickUp | Tasks, subtasks, assignees; time as comments or time entries. AC: contract tests green | S02, S03 | L | — | — | API |
| E23-S06 | GitHub and GitLab | Issues as tasks; time as comments or GitLab time tracking. AC: org-level install | S02, S03 | L | — | — | API |
| E23-S07 | Slack and Microsoft Teams | Notifications to channels and DMs; slash command for timer status. AC: per-org install with scoped tokens | S01, E21 | M | — | — | API |
| E23-S08 | Integrations UI | Catalog, connect, link projects and users, sync status and errors. AC: reconnect prompt on `needs_reauth` | S01 | M | — | integrations/\* | Web |
| E23-S09 | QuickBooks Online and Xero | Time and invoice export with customer and item mapping. AC: exported time locked | S01, E29 | L | — | — | API, Web |
| E23-S10 | BambooHR | Import employees, time-off policies, requests, balances (read-only in Worklume). AC: 15-minute sync | S01, E26 | M | — | — | API |
| E23-S11 | Google and Microsoft calendars | Meetings for Insights; shift export. AC: only free/busy and titles when allowed | S01 | M | — | — | API |

## E24 · Mobile app

| Field | Value |
| --- | --- |
| Purpose | Time tracking, approvals and requests on iOS and Android; GPS comes in E27 |
| Depends on | E02-S09, E07, E08, E19 |
| Features | Shell and auth; offline outbox and sync; timer; timesheets and requests; push; notice; store pipeline; device testing |
| Tables | — (uses existing) |
| APIs | /oauth/*, /v1/desktop/register, config, sync, heartbeat; /v1/timer*, /v1/timesheets\*, /v1/time-off\* |
| Frontend | — |
| Desktop | — |
| Mobile | Everything listed |
| Tests | Detox/Maestro E2E; offline scenarios; iOS and Android device farm |
| Release | V1 |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E24-S01 | Shell and authentication | Expo dev build, PKCE with app links, Keychain/Keystore, DPoP, org switch. AC: biometric unlock optional | E02-S09 | M | — | /oauth/\* | Mobile |
| E24-S02 | Offline outbox and sync | SQLite outbox, same envelope as desktop, background sync windows. AC: a day offline syncs with no duplicates | S01, E07-S03 | L | — | desktop/sync | Mobile |
| E24-S03 | Timer | Start, stop, switch, breaks, project/task picker, today total. AC: timer state matches desktop within 2 s via WebSocket | S02 | M | — | timer/\* | Mobile |
| E24-S04 | Timesheets and requests | View and submit timesheets; manual-time and time-off requests; manager approvals. AC: approve from a push notification | S03, E19 | M | — | timesheets\*, manual-time-requests\*, time-off\* | Mobile |
| E24-S05 | Push | Register tokens; deep links from notifications. AC: token refreshed on reinstall | S01, E21-S02 | S | push\_tokens | — | Mobile |
| E24-S06 | Notice and privacy screen | Acknowledge notice; show what is collected on mobile. AC: tracking blocked until acknowledged | S01, E05-S04 | S | consent\_records | notice/acknowledge | Mobile |
| E24-S07 | Store pipeline | EAS build and submit, TestFlight, Play internal track, staged rollout. AC: release from a tag with no manual steps | S01 | M | — | — | Mobile, Infra |
| E24-S08 | Device and battery testing | Real-device farm across iOS and Android versions and OEMs. AC: < 2% battery/hour while tracking without GPS | S03 | M | — | — | Mobile |

## E35 · Browser extension

| Field | Value |
| --- | --- |
| Purpose | Web timer in the browser and a reliable URL source where the OS cannot provide one (Wayland, some X11 setups) |
| Depends on | E08, E09 |
| Features | MV3 extension; native-messaging bridge to the agent; store publishing and enterprise policy |
| Tables | — |
| APIs | /v1/timer\*, /v1/projects |
| Frontend | Extension popup UI |
| Desktop | Native-messaging host in the agent |
| Mobile | — |
| Tests | Playwright extension tests; bridge message schema tests |
| Release | V1 |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E35-S01 | MV3 extension with timer | Chrome, Edge, Firefox; sign-in, timer, project picker. AC: shares auth with the web app securely | E08 | M | — | timer/\* | Ext |
| E35-S02 | Native-messaging URL bridge | Extension reports active-tab domain (or path if allowed) to the local agent only; never to the server directly. AC: agent on Wayland shows URL usage when the extension is installed | S01, E09-S12 | L | — | — | Ext, Desktop |
| E35-S03 | Store publishing and enterprise install | Chrome Web Store, Edge Add-ons, AMO; force-install policy docs. AC: listed in all three stores | S01 | S | — | — | Ext |

## E25 · Scheduling and attendance

| Field | Value |
| --- | --- |
| Purpose | Planned shifts, and whether people showed up for them |
| Depends on | E08, E21 |
| Features | Shift series and instances; shift evaluation and alerts; attendance records; schedule calendar; live board and reports |
| Tables | shift\_series, shifts, attendance\_records |
| APIs | /v1/schedules/\*, /v1/attendance, /v1/attendance/today |
| Frontend | Schedule calendar (month/week/day), attendance board |
| Desktop | Shift-only tracking and automatic start (E08-S11) |
| Mobile | My shifts; shift reminders |
| Tests | Recurrence across DST; late/missed/abandoned evaluation; timezone per member |
| Release | V2 |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E25-S01 | Shift series and instances | RRULE series materialized 8 weeks ahead; edit this/following/all; overlap checks. AC: DST weeks keep local start times | E08 | L | shift\_series, shifts | schedules/shifts\* | API |
| E25-S02 | Shift evaluation and alerts | Grace periods; on time, early, late, missed, abandoned; alerts and webhooks. AC: status final within 5 minutes of shift end | S01, E21-S03 | L | shifts | — | API |
| E25-S03 | Attendance records | Daily record per member (present, late, partial, absent, leave, holiday). AC: recomputed when time or leave changes | S02 | M | attendance\_records | attendance | API |
| E25-S04 | Schedule calendar UI | Create, copy, drag, bulk-assign shifts; team filter. AC: 100 shifts created in one action | S01 | L | — | schedules/\* | Web |
| E25-S05 | Live board and attendance reports | Today board; Shift Attendance report. AC: board updates live | S03, E18 | M | — | attendance/today, reports | API, Web |

## E26 · Breaks, time off, holidays and overtime

| Field | Value |
| --- | --- |
| Purpose | Workforce rules that turn raw time into payable time |
| Depends on | E08, E19 |
| Features | Break policies; time-off ledger and accruals; requests and balances; holidays; overtime; related reports |
| Tables | break\_policies, break\_policy\_members, work\_breaks, time\_off\_policies, time\_off\_policy\_members, time\_off\_ledger, time\_off\_balances, time\_off\_requests, holiday\_calendars, holidays, holiday\_assignments, overtime\_policies, overtime\_policy\_members, overtime\_calculations |
| APIs | /v1/attendance/break-policies\*, /v1/timer/breaks/*, /v1/time-off/*, /v1/payroll/overtime-policies\* |
| Frontend | Break settings; time-off policies, requests, balances; holiday calendars; overtime settings |
| Desktop | Break button and break limits |
| Mobile | Break button; time-off requests |
| Tests | Accrual idempotency (re-running a period adds nothing); balance = ledger sum; overtime thresholds weekly and daily |
| Release | V2 (breaks may ship in V1 if demanded) |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E26-S01 | Breaks | Policies (paid/unpaid, rules), break start/stop on all clients, abandoned-break alert. AC: unpaid breaks excluded from payable time | E08 | M | break\_policies, break\_policy\_members, work\_breaks | break-policies\*, timer/breaks/\* | API, Web, Desktop, Mobile |
| E26-S02 | Time-off ledger and accruals | Append-only ledger, accrual job per policy type, rollover and caps. AC: re-running an accrual period changes nothing | E04 | XL | time\_off\_\* | time-off/ledger, adjustments | API |
| E26-S03 | Requests and balances | Request, approve, deny, cancel; balance and pending views. AC: approved leave appears on timesheets | S02, E19 | M | time\_off\_requests, time\_off\_balances | time-off/requests\*, balances | API, Web, Mobile |
| E26-S04 | Holidays | Preset calendars (starting with India, US, UK, EU countries), custom calendars, assignments. AC: holiday pay appears in amounts owed | S02 | M | holiday\_calendars, holidays, holiday\_assignments | holiday-calendars\* | API, Web |
| E26-S05 | Overtime | Weekly and daily thresholds, multipliers, double time, frozen on approval. AC: matches a hand-calculated test set of 50 cases | E19 | L | overtime\_\* | overtime-policies\* | API, Web |
| E26-S06 | Workforce reports | Time Off Balances, Time Off Transactions, Work Breaks, Overtime. AC: golden files | S03, S05, E18 | M | — | reports | API |

## E27 · Locations and job sites

| Field | Value |
| --- | --- |
| Purpose | GPS tracking modes and geofenced job sites for field teams |
| Depends on | E24 |
| Features | Background location; location ingest and map; job sites; arrival/departure actions; visits and reports; office networks |
| Tables | location\_points, job\_sites, job\_site\_members, job\_site\_visits, office\_networks |
| APIs | /v1/attendance/locations, /v1/attendance/job-sites\*, /v1/attendance/job-site-visits, /v1/attendance/office-networks\* |
| Frontend | Live map, member route, job sites editor |
| Desktop | Network identity for office detection |
| Mobile | Background location and geofencing |
| Tests | Battery budget; geofence enter/exit simulation; iOS 20-region rotation |
| Release | V2 (Locations add-on) |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E27-S01 | Background location module | Choose Transistorsoft or expo-location after a spike; modes off/timer/shift/always; permissions flows. AC: < 3% battery/hour in "timer" mode on reference devices | E24 | XL | — | desktop/sync `location.batch` | Mobile |
| E27-S02 | Location ingest and map | Partitioned points, simplified paths, live map. AC: a day's route renders in < 1 s | S01 | L | location\_points | attendance/locations | API, Web |
| E27-S03 | Job sites and geofences | PostGIS sites; nearest-20 (iOS) / 100 (Android) region registration. AC: registration refreshes as the member moves | S01 | L | job\_sites, job\_site\_members | job-sites\* | API, Web, Mobile |
| E27-S04 | Arrival and departure actions | Start/stop timer, remind, restrict tracking to sites. AC: action fires within 2 minutes of entry | S03 | L | job\_site\_visits | job-site-visits | Mobile, API |
| E27-S05 | Visits and job reports | Job Attendance and Job Site Visits reports. AC: golden files | S04, E18 | M | — | reports | API |
| E27-S06 | Office networks | Network identity from agents, auto-detection (3+ members), confirm UI, remote-vs-office attendance. AC: no GPS used | E09 | M | office\_networks | office-networks\* | API, Web, Desktop |

## E28 · Expenses

| Field | Value |
| --- | --- |
| Purpose | Capture, approve and bill or reimburse expenses |
| Depends on | E06; E29 for reimbursement and invoicing |
| Features | Categories and expenses with receipts; approval flow; mobile receipt capture; report |
| Tables | expense\_categories, expenses, expense\_receipts |
| APIs | /v1/expenses\* |
| Frontend | Expenses list, form, approvals |
| Desktop | — |
| Mobile | Receipt photo capture |
| Tests | Receipt upload abuse (type, size, malware scan); state machine |
| Release | V2 |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E28-S01 | Expenses and receipts | Categories, CRUD, presigned receipt upload, malware scan. AC: infected files quarantined | E06 | M | expense\_\* | expenses\* | API, Web |
| E28-S02 | Approval flow | Submit, approve, reject; no self-approval. AC: approved billable expenses available to invoices | S01 | M | expenses | submit, approve, reject | API, Web |
| E28-S03 | Mobile receipt capture | Camera capture, crop, upload. AC: works offline and uploads later | S01, E24 | M | expense\_receipts | receipts/upload-url | Mobile |
| E28-S04 | Expenses report | AC: filters by project, member, category, billing type | S02, E18 | S | — | reports | API |

## E29 · Payroll and invoicing

| Field | Value |
| --- | --- |
| Purpose | Turn approved time into money owed, paid and invoiced |
| Depends on | E19, E06-S05; E26 for overtime, PTO and holidays |
| Features | Amounts-owed engine; payroll runs; payout providers; payslips; client invoices; recurring invoices; team invoices; external payroll export |
| Tables | payout\_accounts, payroll\_runs, payments, payment\_line\_items, payslips, invoices, invoice\_lines, invoice\_payments, recurring\_invoice\_schedules, invoice\_sequences |
| APIs | /v1/payroll/*, /v1/invoices* |
| Frontend | Amounts owed, payroll runs, payments, invoices |
| Desktop | — |
| Mobile | Own payslips (view) |
| Tests | Money arithmetic property tests; idempotent provider calls; gap-free invoice numbers under concurrency |
| Release | V1 (client invoices), V2 (payroll, payouts, team invoices) |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E29-S01 | Amounts-owed engine | Rates × approved time + overtime + PTO + holidays + expenses, from base tables, per currency. AC: 200 hand-checked cases match exactly | E19, E06-S05 | XL | — | payroll/amounts-owed | API |
| E29-S02 | Payroll runs | Draft, approval window, approve (step-up), cancel; automatic runs per pay schedule. AC: a period cannot be paid twice | S01 | L | payroll\_runs, payments, payment\_line\_items | payroll/runs\* | API, Web |
| E29-S03 | Payout providers | Wise and PayPal first, then Deel, Gusto, Payoneer, Remote; payout accounts; idempotent payment calls; status webhooks. AC: provider retry never double-pays | S02 | XL | payout\_accounts, payments | payroll/payments\*, payout-accounts\* | API, Web |
| E29-S04 | Payslips and payment reports | PDF payslips; Amounts Owed and Payments reports. AC: member can download own payslip | S02, E18 | M | payslips | payments/{id}/payslip | API, Web, Mobile |
| E29-S05 | Client invoices | From billable time and expenses; gap-free numbering; send; PDF; record payments; aging report. AC: sent invoice locks its time | E06-S05 | L | invoices, invoice\_lines, invoice\_payments, invoice\_sequences | invoices\* | API, Web |
| E29-S06 | Recurring invoices and reminders | Weekly/biweekly/monthly schedules; payment reminders; PayPal payment link. AC: schedule honours org timezone | S05 | M | recurring\_invoice\_schedules | invoices/recurring\* | API, Web |
| E29-S07 | Team (contractor) invoices | Members invoice the org from approved time. AC: only own invoices visible to members | S05, E19 | M | invoices | invoices\* | API, Web |
| E29-S08 | External payroll export | Mark as paid; export CSV for outside payroll systems. AC: exported periods locked | S01 | S | payments | mark-paid, reports | API, Web |

## E30 · Enterprise identity and administration

| Field | Value |
| --- | --- |
| Purpose | What large customers require before signing |
| Depends on | E02, E04, E09 |
| Features | SSO; SCIM; custom roles; audit log UI and streaming; silent MDM agent; IT admin tools; enterprise billing; break-glass tool |
| Tables | sso\_connections, organization\_domains, user\_identities, scim\_tokens, platform\_access\_grants (+ roles for custom roles) |
| APIs | /v1/auth/sso/*, /scim/v2/*, /v1/roles\*, /v1/audit-logs\* |
| Frontend | SSO setup wizard, SCIM tokens, role editor, audit log viewer |
| Desktop | Silent/MDM packages (E10-S09, E11-S08, Linux) |
| Mobile | SSO sign-in |
| Tests | SAML security suite (signature wrapping, replay, audience); SCIM compliance tests; custom role cannot exceed creator |
| Release | V3 |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E30-S01 | SSO (SAML and OIDC) | Build vs WorkOS decision; domain verification; SP-initiated flows; enforce SSO; JIT provisioning. AC: passes a SAML security test suite; owners keep a break-glass passkey | E02 | XL | sso\_connections, organization\_domains, user\_identities | auth/sso/\* | API, Web |
| E30-S02 | SCIM 2.0 | Users and Groups (→ teams); deprovision suspends and revokes. AC: passes Okta and Entra ID SCIM validators | S01 | L | scim\_tokens | /scim/v2/\* | API |
| E30-S03 | Custom roles | Role editor from permission catalog; no escalation. AC: a custom role cannot include owner-only permissions | E04-S03 | M | roles, role\_permissions | roles\* | API, Web |
| E30-S04 | Audit log UI, export, streaming | Viewer with filters; CSV/PDF export; streaming to S3 or webhook for SIEM. AC: hash chain verifiable by the customer | E01-S03 | M | audit\_logs | audit-logs\* | API, Web |
| E30-S05 | Silent MDM agent | Silent mode policy with lawful-basis attestation; macOS pkg + PPPC, Windows MSI (Intune), Linux package. AC: silent devices labelled in admin views | E09, E10-S09, E11-S08 | L | devices, policy\_values | desktop/config | Desktop, API, Web |
| E30-S06 | IT admin tools | IT Admin role, device fleet view, bulk commands, version compliance. AC: outdated agents highlighted | E07-S07 | S | — | devices\* | Web |
| E30-S07 | Enterprise billing | Invoiced annual contracts, custom entitlement overrides. AC: override expiry honoured | E20 | M | entitlements | billing/\* | API, Web |
| E30-S08 | Break-glass access tool | Staff admin app with time-limited, approved, audited tenant access. AC: every staff read lands in the tenant's audit log | E01-S03 | M | platform\_access\_grants | staff admin | API, Web |

## E31 · Privacy operations

| Field | Value |
| --- | --- |
| Purpose | Operational privacy: rights requests, retention control, aggregate-only mode, residency |
| Depends on | E14-S06, E15-S05 |
| Features | Data subject requests; retention settings and legal holds; aggregate-only mode; second regional cell; legal documents flow; cancellation purge |
| Tables | data\_subject\_requests, legal\_holds, retention\_runs |
| APIs | /v1/privacy/requests\*, /v1/privacy/legal-holds\*, retention policies under /v1/organizations/current/policies |
| Frontend | Privacy centre for admins and members |
| Desktop / Mobile | "Request my data" link |
| Tests | Export completeness test (every table with the member's data is included); erasure leaves only pseudonymised payroll records |
| Release | V2 (retention settings, aggregate-only), V3 (DSR tooling, second cell) |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E31-S01 | Data subject requests | Access/export bundle (JSON, CSV, screenshot ZIP), rectify, erase with pseudonymisation of legally retained records, deadlines. AC: export covers all member data classes | E14-S06 | XL | data\_subject\_requests | privacy/requests\* | API, Web |
| E31-S02 | Retention settings and legal holds | Per data class, ≤ plan maximum; holds pause deletion. AC: a hold stops screenshot expiry including the S3 backstop | E15-S05 | M | legal\_holds, policy\_values | privacy/legal-holds\*, policies | API, Web |
| E31-S03 | Aggregate-only mode | Disable member-level activity, app and screenshot views org-wide. AC: every member-level endpoint returns 403 `aggregate_only` | E04-S03 | M | policy\_values | many | API, Web |
| E31-S04 | Second regional cell | Terraform cell module, region choice at signup, org export/import tool. AC: an org can be moved between cells in a rehearsal | E34-S06 | XL | — | — | Infra, API |
| E31-S05 | Legal documents flow | DPA acceptance, sub-processor page, DPIA template download. AC: DPA version recorded per org | E03 | S | — | — | Web |
| E31-S06 | Cancellation purge | Hard-delete pipeline at `purge_after` across database, S3 and analytics store. AC: purge report proves zero remaining rows and objects | E20-S05 | M | organizations, retention\_runs | — | API, Infra |

## E32 · Insights analytics

| Field | Value |
| --- | --- |
| Purpose | Workforce analytics beyond reports: focus, utilization, meetings, classification by job type |
| Depends on | E16-S05, E18, E23-S11 (calendars) |
| Features | Analytics store and pipeline; focus and utilization; meetings; Insights dashboards; job-type templates; ClickHouse; benchmarks |
| Tables | Analytics store schema (outside the OLTP database) |
| APIs | /v1/insights/\* (widgets) |
| Frontend | Insights dashboards |
| Desktop / Mobile | — |
| Tests | Metric definitions tested against fixtures; tenant filter on every analytics query |
| Release | V3 (Insights add-on); benchmarks Future |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E32-S01 | Analytics store and pipeline | Separate PostgreSQL analytics instance fed by outbox/CDC export. AC: < 15 min lag; no path from analytics back to OLTP | E01-S02 | L | analytics | — | Infra, API |
| E32-S02 | Focus and utilization | Focus sessions (≥ 30 min), utilization vs expected hours per job type. AC: definitions documented in-app | S01 | M | analytics | insights/\* | API |
| E32-S03 | Meetings analytics | Meeting load and cost from calendars. AC: event titles not stored unless allowed | S01, E23-S11 | M | analytics | insights/\* | API |
| E32-S04 | Insights dashboards | Widgets: utilization, focus, classification, timeline, leaderboard (optional), remote vs office. AC: team leads see their team only | S02 | L | — | insights/\* | Web |
| E32-S05 | Job-type templates | Default classifications per job type. AC: new org gets sensible defaults | E16-S05 | M | productivity\_classifications | classifications | API |
| E32-S06 | ClickHouse migration | Stage C: analytics store to ClickHouse with row policies. AC: Insights p95 < 1 s for 1,000-member orgs | S01 | XL | analytics | — | Infra |
| E32-S07 | Benchmarks | Anonymised cross-tenant medians by job type (opt-in, k-anonymity ≥ 10 orgs). AC: no benchmark from fewer than 10 orgs | S06 | L | analytics | insights/benchmarks | API, Web |

## E33 · AI

| Field | Value |
| --- | --- |
| Purpose | Anomalies, summaries, natural-language reporting, workload analysis and recommendations (Architecture §18) |
| Depends on | E32-S01, E18-S01, E09-S13 |
| Features | AI plane; anomaly rules; automated-input detection; summaries; NL reporting; workload and recommendations; classification suggestions; evaluation |
| Tables | ai\_model\_versions, ai\_insights, ai\_anomalies, ai\_queries, ai\_usage\_ledger |
| APIs | /v1/ai/*, /internal/ai/* |
| Frontend | Insight feed, anomaly review, ask-a-question panel, AI settings |
| Desktop | Input features (E09-S13); "my insights" view |
| Mobile | Insight feed (read) |
| Tests | Golden datasets; number-validator tests; prompt-injection tests; permission-scoped NL queries; shadow comparisons |
| Release | V3 (AI add-on) |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E33-S01 | AI plane foundation | Separate account, Python service, Bedrock access, internal endpoints (mTLS), model registry, quotas, org AI switch. AC: AI plane has no OLTP credentials | E32-S01 | L | ai\_model\_versions, ai\_usage\_ledger | /internal/ai/\*, ai/settings, ai/usage | AI, API, Infra |
| E33-S02 | Anomaly rules and review | Versioned rules, suspicious-app list, confidence levels, explanation, review UI. AC: nothing is auto-penalised; subject can see anomalies about them if policy allows | S01 | L | ai\_anomalies | ai/anomalies\* | AI, API, Web |
| E33-S03 | Automated-input detector | Model on input-timing features with labelled jiggler/auto-clicker data. AC: precision ≥ 95% on held-out set before activation | S02, E09-S13 | XL | ai\_anomalies | — | AI |
| E33-S04 | Summaries | Facts JSON from rollups, fixed prompt, number validator, audience rules. AC: 0 unvalidated numbers in 1,000 generated summaries | S01 | L | ai\_insights | ai/insights\* | AI, API, Web |
| E33-S05 | Natural-language reporting | Tool-calling over report query classes, permission-checked plans, streaming answers with citations. AC: questions about out-of-scope members refused | S01, E18-S01 | XL | ai\_queries | ai/queries\* | AI, API, Web |
| E33-S06 | Workload and recommendations | Deterministic utilization, overtime trend, budget burn; rule-generated recommendations phrased by LLM. AC: every recommendation links its evidence | S04 | L | ai\_insights | ai/insights | AI, API, Web |
| E33-S07 | Classification suggestions | Suggest ratings for unclassified apps/domains per job type; admin approval. AC: suggestions never applied automatically | S01, E16-S05 | M | productivity\_classifications | ai/classification-suggestions | AI, API, Web |
| E33-S08 | Evaluation and shadow mode | Golden sets, offline evals, shadow runs before activating a new model or prompt version. AC: activation blocked on failing evals | S01 | L | ai\_model\_versions | — | AI |

## E34 · Scale and resilience

| Field | Value |
| --- | --- |
| Purpose | Stay fast and available from Stage A to Stage D (Architecture §22) |
| Depends on | Continuous; most stories triggered by thresholds |
| Features | Pooling; load testing and capacity model; telemetry database split; ingest buffer; DR; cell automation; cost dashboards; sharding |
| Tables | — (moves telemetry tables) |
| APIs | — |
| Frontend / Desktop / Mobile | — |
| Tests | k6 load suites (10k and 100k simulated agents); chaos tests; restore drills |
| Release | Load suite before Public Beta; the rest by threshold |

| ID | Title | Description · acceptance criteria | Deps | Size | Tables | APIs | Apps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E34-S01 | Connection pooling | PgBouncer (transaction mode) with `SET LOCAL` compatibility verified. AC: RLS tests pass through the pooler | E03-S04 | M | — | — | Infra |
| E34-S02 | Load testing and capacity model | k6 agent simulator, dashboards, capacity model per stage. AC: 10,000 simulated agents at SLO before Public Beta | E07-S08 | L | — | desktop/\* | Infra |
| E34-S03 | Telemetry database split | Move leaf telemetry tables to a dedicated cluster. AC: zero-downtime cut-over rehearsal | S02 | XL | telemetry tables | — | Infra, API |
| E34-S04 | SQS ingest buffer | Accept-then-process ingest under load spikes. AC: no data loss under 5× spike | S02 | L | sync\_batches | desktop/sync | Infra, API |
| E34-S05 | DR drills and runbooks | Quarterly restore, region-failure runbook. AC: RTO ≤ 4 h demonstrated | E00-S03 | M | — | — | Infra |
| E34-S06 | Cell automation | Terraform module to stand up a full cell. AC: new cell in < 1 day | E00-S03 | L | — | — | Infra |
| E34-S07 | Cost dashboards | Cost per seat by component (S3, RDS, compute, AI). AC: weekly report | E00-S05 | S | — | — | Infra |
| E34-S08 | Telemetry sharding | Citus or equivalent by `organization_id` at Stage D. AC: design doc and rehearsal | S03 | XXL (split) | telemetry tables | — | Infra |
