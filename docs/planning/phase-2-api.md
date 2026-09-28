# REST API — endpoint catalog

Version 1, JSON over HTTPS at `https://api.worklume.com/v1`. Every token is bound to one organization, so no endpoint takes an organization ID from the client for tenant data.

## Conventions (apply to every endpoint)

Each endpoint table below lists only what differs from these defaults.

| Topic | Rule |
| --- | --- |
| Authentication | `Authorization: Bearer <access token>` (JWT, 15 min). Desktop and mobile also send a `DPoP` proof header. Unless a section says otherwise, the token must be org-bound and the member active. |
| Authorization | "Permission" column = the RBAC permission checked, plus its scope (own / team / project / all). A missing permission gives 403; a resource outside the caller's scope gives 404. |
| Organization context | Always taken from the token. No endpoint accepts an organization ID for tenant data. |
| Format | JSON, `snake_case`. Timestamps UTC ISO 8601; dates `YYYY-MM-DD` in the org timezone; money `{amount_minor, currency}`; durations in seconds. |
| Single responses | `{"data": {…}}` |
| Lists | `{"data": […], "meta": {"next_cursor": "…", "has_more": true}}`. `limit` default 50, max 200; `cursor` opaque; `sort=-started_at`; filters as `filter[field]=`; `include=project,task` for related objects. |
| Validation | Laravel FormRequests; failures return 422 with per-field errors. Date ranges are limited per endpoint (e.g. 31 days for raw activity). Unknown fields are rejected. |
| Concurrency | Mutable resources return `ETag` (the row version). `PATCH` and state changes accept `If-Match`; a stale version returns 412. |
| Idempotency | Column values: **natural** (GET, PUT, DELETE are safe to repeat), **key required** (send `Idempotency-Key`; replays within 24 h return the stored response; the same key with a different body returns 422 `idempotency_key_reused`), **key optional**, **client ID** (the body's client-generated UUID makes it idempotent). |
| Rate limits | Per token 600/min, per org 10,000/min unless stated; headers `RateLimit-Limit`, `RateLimit-Remaining`, `RateLimit-Reset`; 429 with `Retry-After`. |
| Versioning | `/v1` in the path; additive changes only within v1; breaking changes in v2 with `Deprecation` and `Sunset` headers and 12 months' notice. |

**Error format** (RFC 9457, `application/problem+json`):

```json
{
  "type": "https://docs.worklume.com/errors/validation_failed",
  "title": "Validation failed",
  "status": 422,
  "code": "validation_failed",
  "detail": "ended_at must be after started_at",
  "errors": [{"field": "ended_at", "code": "after", "message": "must be after started_at"}],
  "request_id": "0192f0a4-…"
}
```

| HTTP | Common codes |
| --- | --- |
| 400 | `invalid_request`, `malformed_json` |
| 401 | `unauthenticated`, `token_expired`, `token_revoked`, `dpop_invalid` |
| 403 | `permission_denied`, `step_up_required`, `mfa_enrolment_required`, `entitlement_required`, `org_suspended`, `quota_exceeded`, `ai_disabled` |
| 404 | `not_found` (also for resources in another org or outside your scope) |
| 409 | `state_conflict`, `timer_already_running`, `overlap`, `integration_managed`, `duplicate` |
| 412 | `version_mismatch` |
| 413 | `payload_too_large` |
| 422 | `validation_failed`, `idempotency_key_reused` |
| 423 | `time_locked` (approved, paid, invoiced or exported time) |
| 429 | `rate_limited` |
| 5xx | `internal_error`, `service_unavailable` (safe to retry with the same idempotency key) |

## /auth

Public endpoints are marked "none" in the Permission column; they are rate-limited per IP and per account. Endpoints that act on a user without an org yet accept an identity-only token (`org` claim empty).

| Method | Path | Permission | Request | Response | Validation · errors | Idempotency |
| --- | --- | --- | --- | --- | --- | --- |
| POST | /v1/auth/register | none | name, email, password, organization\_name?, locale, timezone | 201 user; verification email sent | password ≥ 12 chars and not breached; 422 `email_taken` (generic message to avoid enumeration) | key optional |
| POST | /v1/auth/verify-email | none | token | 204 | 422 `token_invalid` or `token_expired` | natural |
| POST | /v1/auth/login | none | email, password, client\_id | 200 tokens, or `{mfa_required, challenge_id, methods}` | 401 `invalid_credentials`; 403 `sso_required` (with SSO URL); 423 `account_locked` | natural |
| POST | /v1/auth/mfa/verify | challenge | challenge\_id, method (totp, email, passkey, recovery), code or WebAuthn assertion | 200 tokens | 401 `mfa_invalid`; 5 attempts then challenge void | natural |
| POST | /v1/auth/passkeys/options | none | email? | 200 WebAuthn request options | — | natural |
| POST | /v1/auth/passkeys/verify | none | assertion | 200 tokens | 401 `passkey_invalid`; sign-count regression → 401 and alert | natural |
| POST | /v1/auth/email-code | none | email | 202 (always, no enumeration) | 10 codes / 30 min | natural |
| POST | /v1/auth/email-code/verify | none | email, code | 200 tokens or MFA challenge | 401 `code_invalid`; code valid 5 min | natural |
| POST | /v1/auth/password/forgot | none | email | 202 | — | natural |
| POST | /v1/auth/password/reset | none | token, password | 204; all sessions revoked | 422 `token_invalid`, weak password | natural |
| POST | /v1/auth/refresh | refresh token (cookie or body) + DPoP | refresh\_token? | 200 new access + rotated refresh | 401 `token_reused` (family revoked), `token_expired`, `dpop_invalid` | natural (rotation) |
| POST | /v1/auth/switch-organization | identity | organization\_id | 200 tokens for that org | 404 if not a member; 403 `org_suspended` (owners still allowed to billing) | natural |
| GET | /v1/auth/me | any | — | user, organization, member, effective permissions, entitlements, policy version | — | natural |
| POST | /v1/auth/logout | any | — | 204; session revoked | — | natural |
| GET | /v1/auth/sessions | own | — | list of sessions (client, device, IP, last seen) | — | natural |
| DELETE | /v1/auth/sessions/{id} | own | — | 204 | 404 | natural |
| POST | /v1/auth/sessions/revoke-all | own; step-up | except\_current? | 204 | 403 `step_up_required` | natural |
| POST | /v1/auth/step-up | any | method, code or assertion | 200 `{mfa_at}` | 401 `mfa_invalid` | natural |
| POST | /v1/auth/mfa/totp | own | label | 200 `{factor_id, otpauth_uri}` (unconfirmed) | max 5 factors | key optional |
| POST | /v1/auth/mfa/totp/{id}/confirm | own | code | 200 + recovery codes if first factor | 422 `code_invalid` | natural |
| DELETE | /v1/auth/mfa/{factor\_id} | own; step-up | — | 204 | 409 `last_factor_required` when org enforces MFA | natural |
| POST | /v1/auth/mfa/recovery-codes | own; step-up | — | 200 ten new codes (old ones void) | — | key required |
| POST | /v1/auth/passkeys/register/options | own | — | 200 WebAuthn creation options | — | natural |
| POST | /v1/auth/passkeys | own | attestation, name | 201 passkey | 422 `attestation_invalid` | natural (credential ID unique) |
| DELETE | /v1/auth/passkeys/{id} | own; step-up | — | 204 | — | natural |
| GET | /v1/auth/sso/discover | none | email (query) | `{sso: bool, start_url}` | — | natural |
| GET | /v1/auth/sso/{connection\_id}/start | none | redirect\_uri, PKCE params | 302 to IdP | 404 inactive connection | natural |
| POST | /v1/auth/sso/saml/acs | IdP | SAMLResponse, RelayState | 302 back to client with code | signature, audience, time window, replay (assertion ID) checks; 401 `sso_failed` | natural (assertion IDs single-use) |
| GET | /v1/auth/sso/oidc/callback | IdP | code, state | 302 back to client | state and nonce checks | natural |

**OAuth / OIDC endpoints** (standard behaviour): `GET /oauth/authorize` (code + PKCE S256 only), `POST /oauth/token` (authorization\_code, refresh\_token; DPoP supported), `POST /oauth/revoke`, `POST /oauth/introspect` (confidential clients), `GET /.well-known/openid-configuration`, `GET /.well-known/jwks.json`.

**SCIM 2.0** (bearer SCIM token, Enterprise): `/scim/v2/Users` and `/scim/v2/Groups` with GET, POST, PUT, PATCH, DELETE per RFC 7644; `/scim/v2/ServiceProviderConfig`, `/Schemas`, `/ResourceTypes`. DELETE or `active=false` suspends the member and revokes sessions.

## /organizations, /users, /teams

In this API, "users" are the members of the current organization (`organization_members`); global login identities are never listed.

| Method | Path | Permission | Request | Response | Validation · errors | Idempotency |
| --- | --- | --- | --- | --- | --- | --- |
| GET | /v1/organizations | identity | — | orgs where the caller has a membership (id, name, role, status) | — | natural |
| POST | /v1/organizations | identity | name, timezone, currency, country\_code, data\_region | 201 org; caller becomes owner; trial starts | 422 unknown timezone/region | key required |
| GET | /v1/organizations/current | any member | — | org profile, settings, trial/plan summary | — | natural |
| PATCH | /v1/organizations/current | `organization.manage` | name, timezone, currency, week\_start, locale | 200 | changing timezone after 30 days needs owner; 412 | natural |
| DELETE | /v1/organizations/current | owner; step-up | confirm\_name | 202; status `archived`, purge scheduled | 422 name mismatch | natural |
| POST | /v1/organizations/current/transfer-ownership | owner; step-up | member\_id | 200 | target must be active member; 409 `state_conflict` | key required |
| GET | /v1/organizations/current/policies | `policies.view` | scope\_type?, scope\_id? | effective values with source (default/org/team/member/project) | — | natural |
| PUT | /v1/organizations/current/policies/{key} | `policies.manage` | scope\_type, scope\_id?, value, effective\_from? | 200; `settings_version` bumped | JSON-schema validation; 403 `entitlement_required` | natural |
| DELETE | /v1/organizations/current/policies/{key} | `policies.manage` | scope\_type, scope\_id | 204 (falls back to parent) | — | natural |
| GET/POST/PATCH/DELETE | /v1/roles, /v1/roles/{id} | `roles.manage` | key, name, scope\_level, permissions\[\] | role with permissions | cannot exceed caller's permissions; system roles read-only (409) | natural / key optional on POST |
| GET/POST/PATCH/DELETE | /v1/job-types, /v1/custom-fields | `organization.manage` | name, fields | — | unique names | natural |
| GET | /v1/users | `members.view` (all) or team scope | filter\[status, team\_id, role, job\_type\_id\], q | members list (rates only with `rates.view`) | — | natural |
| GET | /v1/users/me | own | — | own member profile | — | natural |
| PATCH | /v1/users/me | own | display\_name, timezone, notification defaults | 200 | — | natural |
| GET | /v1/users/{member\_id} | `members.view` in scope | include=roles,teams,projects | member | 404 outside scope | natural |
| PATCH | /v1/users/{member\_id} | `members.manage` | job\_title, job\_type\_id, manager\_member\_id, employment\_type, country\_code, tracking\_enabled | 200 | manager cannot be self or create a cycle; 412 | natural |
| PUT | /v1/users/{member\_id}/roles | `roles.assign` | assignments\[\] {role\_id, scope\_type, scope\_id} | 200 | exactly one org role; cannot grant above own level; owner changes need owner | natural |
| DELETE | /v1/users/{member\_id} | `members.manage` | — | 204; seat released; sessions revoked | 409 `last_owner`; 409 unpaid amounts (override flag with step-up) | natural |
| POST | /v1/users/{member\_id}/reactivate | `members.manage` | — | 200 | seat limit may trigger billing | key optional |
| POST | /v1/users/invitations | `members.invite` | invites\[\] (≤ 500) {email, role\_id, team\_ids, project\_roles, pay\_rate?} | 207 per-invite result | duplicate or existing member → per-item error; 429 per-org invite limit | key required |
| GET | /v1/users/invitations | `members.invite` | status | pending invites | — | natural |
| POST | /v1/users/invitations/{id}/resend | `members.invite` | — | 202 | max 5 resends | natural |
| DELETE | /v1/users/invitations/{id} | `members.invite` | — | 204 | — | natural |
| POST | /v1/invitations/accept | identity | token | 200 membership + org-bound tokens | 422 expired/revoked; 409 already member | natural |
| POST | /v1/users/import | `members.invite` | CSV/XLSX file | 202 job id; results via GET /v1/jobs/{id} | ≤ 5,000 rows; header validation | key required |
| GET | /v1/teams | `teams.view` | q | teams | — | natural |
| POST | /v1/teams | `teams.manage` | name, description | 201 | unique name; 409 if SCIM-managed org forbids | key optional |
| GET/PATCH/DELETE | /v1/teams/{id} | `teams.view` / `teams.manage` | name, description | team | SCIM-managed → 409 `scim_managed` | natural |
| PUT | /v1/teams/{id}/members | `teams.manage` | member\_ids\[\] | 200 | members must be active | natural |
| PUT | /v1/teams/{id}/leads | `teams.manage` | leads\[\] {member\_id, permissions\[\]} | 200 (creates team-scope role assignments) | leads must be team members | natural |

## /projects, /tasks (+ clients, rates, budgets)

| Method | Path | Permission | Request | Response | Validation · errors | Idempotency |
| --- | --- | --- | --- | --- | --- | --- |
| GET | /v1/projects | member (own projects) or `projects.view` | filter\[status, client\_id, member\_id\], q | projects | — | natural |
| POST | /v1/projects | `projects.manage` | name, client\_id?, billable\_default, color, record\_activity, member\_ids?, team\_ids? | 201 | unique name; client must be active | key optional |
| GET | /v1/projects/{id} | project member or `projects.view` | include=members,budget | project | 404 outside scope | natural |
| PATCH | /v1/projects/{id} | `projects.manage` or project manager | any editable field | 200 | 412 | natural |
| POST | /v1/projects/{id}/archive, /restore | `projects.manage` | — | 200; running timers on it are stopped | — | natural |
| DELETE | /v1/projects/{id} | `projects.manage`; step-up | — | 204 | 409 `has_time` (archive instead) | natural |
| GET | /v1/projects/{id}/members | project manager or `projects.view` | — | members with project roles | — | natural |
| PUT | /v1/projects/{id}/members | project manager or `projects.manage` | members\[\] {member\_id, project\_role}, team\_ids\[\] | 200 | members active | natural |
| GET/POST | /v1/clients | `clients.view` / `clients.manage` | name, email, address, currency, payment\_terms\_days | clients | unique name | key optional on POST |
| GET/PATCH/DELETE | /v1/clients/{id} (+ /archive, /restore) | `clients.manage` | — | client | DELETE 409 if projects exist | natural |
| GET | /v1/rates | `rates.view` | filter\[member\_id, project\_id, rate\_type\] | rate history | — | natural |
| POST | /v1/rates | `rates.manage`; step-up | member\_id, project\_id?, rate\_type, unit, amount, effective\_from | 201; previous open rate closed the day before | 409 `overlap`; effective\_from cannot be inside a paid period (423) | key required |
| DELETE | /v1/rates/{id} | `rates.manage` | — | 204 | only future-dated rates | natural |
| GET/POST | /v1/budgets | `budgets.view` / `budgets.manage` | subject\_type, project\_id or client\_id, basis, amount, period, thresholds, stop\_timers\_at\_pct | budgets with consumption | one active budget per subject (409) | key optional on POST |
| GET/PATCH/DELETE | /v1/budgets/{id} | same | — | budget | — | natural |
| GET | /v1/budgets/{id}/status | `budgets.view` | period? | consumed, remaining, forecast date | — | natural |
| GET | /v1/tasks | project member | filter\[project\_id, assignee\_id, status\], q | tasks | — | natural |
| POST | /v1/tasks | project member (if org allows) or manager | project\_id, title, description, assignee\_ids, due\_on, estimate\_seconds, parent\_task\_id | 201 | 409 `integration_managed` when the project's tasks come from an integration | key optional |
| GET/PATCH/DELETE | /v1/tasks/{id} | same | editable fields | task | 409 `integration_managed`; DELETE 409 `has_time` → archive | natural |
| POST | /v1/tasks/{id}/complete, /reopen | assignee or manager | — | 200; completion synced to the integration if linked | — | natural |
| PUT | /v1/tasks/{id}/assignees | manager or assignee (self only) | member\_ids\[\] | 200 | assignees must be project members | natural |

## /timer, /time-entries (+ timesheets, manual time requests, idle)

The desktop agent normally reports timer changes through `/v1/desktop/sync`; these endpoints serve the web timer, mobile, the browser extension and third parties.

| Method | Path | Permission | Request | Response | Validation · errors | Idempotency |
| --- | --- | --- | --- | --- | --- | --- |
| GET | /v1/timer | own | — | running timer or `null`, today's total | — | natural |
| POST | /v1/timer/start | own; `time.track` | project\_id, task\_id?, source, device\_id?, client\_event\_id, started\_at? (≤ 60 s in the past) | 201 timer + open time entry | 409 `timer_already_running` (details of other device) unless `takeover: true`; 403 `tracking_disabled`, `limit_reached`, `outside_job_site`; 404 project not assigned | client ID |
| POST | /v1/timer/stop | own | client\_event\_id, stopped\_at? | 200 closed time entry | 409 `no_running_timer` | client ID |
| POST | /v1/timer/switch | own | project\_id, task\_id?, client\_event\_id | 200 new timer (atomic stop + start) | same as start | client ID |
| POST | /v1/timer/breaks/start, /breaks/stop | own | break\_policy\_id, client\_event\_id | 200 break | 403 `break_not_allowed` by policy rule | client ID |
| GET | /v1/timer/team | `time.view` team/all | team\_id? | who is tracking now: member, project, since, device | — | natural |
| GET | /v1/time-entries | own, or `time.view` in scope | filter\[member\_id, project\_id, task\_id, source, billable\], from, to (≤ 92 days) | entries | — | natural |
| POST | /v1/time-entries | own with `time.add_manual`, or `time.edit` in scope | member\_id?, project\_id, task\_id?, started\_at, ended\_at, billable, note, reason | 201 entry, or 202 manual-time request when approval is required | 409 `overlap` (returns conflicting IDs); 423 `time_locked`; limit checks; reason required | key required |
| GET | /v1/time-entries/{id} | own or scope | include=screenshots\_count,activity | entry | — | natural |
| PATCH | /v1/time-entries/{id} | own with `time.edit_own`, or `time.edit` in scope | started\_at, ended\_at, project\_id, task\_id, billable, note, reason | 200 | If-Match; 409 `overlap`; 423 `time_locked`; reason required | natural |
| DELETE | /v1/time-entries/{id} | same | reason | 204; activity, usage and screenshots for that interval are deleted too | 423 `time_locked` | natural |
| POST | /v1/time-entries/{id}/split | same | at, second\_project\_id?, reason | 200 two entries | `at` strictly inside the entry | key required |
| POST | /v1/time-entries/bulk | `time.edit` in scope | ids\[\] (≤ 500), changes {project\_id?, task\_id?, billable?}, reason | 207 per-item results | per-item 423/404 | key required |
| GET | /v1/time-entries/{id}/history | own or scope | — | edit history (who, when, before/after, reason) | — | natural |
| GET | /v1/manual-time-requests | own or `time.approve_manual` | status | requests | — | natural |
| POST | /v1/manual-time-requests/{id}/approve, /deny | `time.approve_manual` in scope | note? | 200 (approve creates the entry) | 409 already decided; 409 `overlap` on approve | natural |
| POST | /v1/manual-time-requests/bulk-approve | same | ids\[\] | 207 | — | key required |
| POST | /v1/manual-time-requests/{id}/cancel | requester | — | 200 | pending only | natural |
| GET | /v1/idle-periods | own | status=pending | idle periods awaiting a decision | — | natural |
| POST | /v1/idle-periods/{id}/resolve | own | resolution (keep, discard, reassign, stop), project\_id? | 200; time entries adjusted | 409 already resolved; policy may force a resolution | natural |
| GET | /v1/timesheets | own or `timesheets.view` in scope | filter\[member\_id, status\], period\_start | timesheets with totals | — | natural |
| GET | /v1/timesheets/{id} | same | include=entries,approvals | timesheet | — | natural |
| POST | /v1/timesheets/{id}/submit | own | note? | 200; period locked for new tracking | 409 not open | natural |
| POST | /v1/timesheets/{id}/unsubmit | own | — | 200 | 409 already approved | natural |
| POST | /v1/timesheets/{id}/approve | `timesheets.approve` in scope | note? | 200 (or next approval step) | cannot approve own (403); 409 state; If-Match | natural |
| POST | /v1/timesheets/{id}/deny | same | reason | 200 | reason required | natural |
| POST | /v1/timesheets/{id}/reopen | `timesheets.approve` all | reason | 200 | 409 if paid | natural |
| POST | /v1/timesheets/bulk-approve | same | ids\[\] | 207 | — | key required |

## /activity, /applications, /websites, /screenshots

All endpoints here also check the privacy policy: in aggregate-only orgs, member-level requests return 403 `aggregate_only`.

| Method | Path | Permission | Request | Response | Validation · errors | Idempotency |
| --- | --- | --- | --- | --- | --- | --- |
| GET | /v1/activity/slots | own, or `activity.view` in scope | member\_id, from, to (≤ 31 days) | 10-minute slots: tracked, active, keyboard, mouse, idle, project, flags | 422 range too long | natural |
| GET | /v1/activity/daily | own or scope | member\_ids\[\] or team\_id, from, to (≤ 366 days) | per member per day: tracked, active %, manual, idle, billable (from rollups) | — | natural |
| GET | /v1/activity/summary | `activity.view` team/all | team\_id?, from, to | averages, distribution, top/bottom members | — | natural |
| GET | /v1/applications | `activity.view` | q | apps seen in this org (catalog entries with icons) | — | natural |
| GET | /v1/applications/usage | own, or `activity.view_apps` in scope | member\_id or team\_id, from, to (≤ 92 days), group\_by (app, member, day, project) | seconds per group, rating | — | natural |
| GET | /v1/websites | `activity.view` | q | domains seen in this org | — | natural |
| GET | /v1/websites/usage | own, or `activity.view_urls` in scope | same as apps; `detail=paths` needs `activity.view_detail` and org path tracking | seconds per domain (or path) | 403 `detail_not_enabled` | natural |
| GET | /v1/classifications | `classifications.view` | filter\[subject\_type, rating, scope\_type, scope\_id\], unclassified=true | rules | — | natural |
| PUT | /v1/classifications | `classifications.manage` | items\[\] {subject\_type, subject\_id or category, scope\_type, scope\_id, rating} (≤ 500) | 200; affected rollups re-queued | — | natural |
| DELETE | /v1/classifications/{id} | `classifications.manage` | — | 204 | — | natural |
| GET | /v1/screenshots | own, or `screenshots.view` in scope | member\_id, from, to (≤ 7 days), project\_id? | metadata + `thumbnail_url` (presigned, 60 s) per screenshot, grouped by slot | — | natural |
| GET | /v1/screenshots/{id} | same | — | metadata + `image_url` (presigned, 60 s); view logged | 404 outside scope or expired | natural |
| DELETE | /v1/screenshots/{id} | owner/admin with `screenshots.delete` (keep\_time), or own block if org allows (remove\_time) | mode, reason? | 204; S3 object deleted; audit logged | 403 if org disabled deletion; 423 when mode=remove\_time and time is locked | natural |
| POST | /v1/screenshots/exports | owner; step-up | member\_ids\[\], from, to (≤ 31 days) | 202 export job (ZIP) | 403 `step_up_required` | key required |

## /attendance, /schedules, /time-off

| Method | Path | Permission | Request | Response | Validation · errors | Idempotency |
| --- | --- | --- | --- | --- | --- | --- |
| GET | /v1/attendance | own, or `attendance.view` in scope | member\_id or team\_id, from, to (≤ 92 days), status? | attendance records per member per day | — | natural |
| GET | /v1/attendance/today | `attendance.view` team/all | team\_id? | live board: present, late, absent, on leave, on break | — | natural |
| GET/POST | /v1/attendance/job-sites | `job_sites.view` / `job_sites.manage` | name, address, lat, lng, radius\_m, project\_id, on\_arrival, on\_departure, restrict\_tracking, member\_ids | job sites | radius ≥ 50 m; 403 `entitlement_required` (Locations) | key optional on POST |
| GET/PATCH/DELETE | /v1/attendance/job-sites/{id} | same | — | job site | — | natural |
| GET | /v1/attendance/job-site-visits | own or `attendance.view` | job\_site\_id?, member\_id?, from, to | visits | — | natural |
| GET | /v1/attendance/locations | own, or `locations.view` in scope | member\_id, from, to (≤ 7 days) | GPS points (simplified path) | 403 `entitlement_required` | natural |
| GET/POST/PATCH/DELETE | /v1/attendance/break-policies(/{id}) | `policies.manage` | name, paid, max\_duration, rule, rule\_params, member\_ids | policies | — | natural / key optional |
| GET/POST/PATCH/DELETE | /v1/attendance/office-networks(/{id}) | `policies.manage` | name, public\_cidr | networks (auto-detected ones need confirming) | valid CIDR | natural |
| GET | /v1/schedules/shifts | own, or `schedules.view` in scope | member\_id or team\_id, from, to (≤ 92 days) | shifts with status | — | natural |
| POST | /v1/schedules/shifts | `schedules.manage` in scope | member\_ids\[\], starts\_at, ends\_at, timezone, minimum\_seconds, grace\_seconds, recurrence? (weekly/biweekly, days, until) | 201 shifts (and series) | ends after start; ≤ 30 days in the past; 409 overlapping shift | key required |
| PATCH | /v1/schedules/shifts/{id} | same | fields; `apply_to` = this, following, all | 200 | 409 past shifts already evaluated | natural |
| DELETE | /v1/schedules/shifts/{id} | same | `apply_to` | 204 | — | natural |
| GET/POST/PATCH | /v1/time-off/policies(/{id}) | `time_off.manage` | name, paid, accrual settings, caps, rollover, allow\_negative, requires\_approval | policies | read-only when `external_source` (409) | natural / key optional |
| PUT | /v1/time-off/policies/{id}/members | `time_off.manage` | members\[\] {member\_id, joined\_on} | 200; accruals back-filled into the ledger | — | natural |
| GET | /v1/time-off/balances | own, or `time_off.view` in scope | member\_id?, policy\_id? | balances + pending | — | natural |
| GET | /v1/time-off/ledger | same | member\_id, policy\_id, from, to | ledger rows | — | natural |
| POST | /v1/time-off/adjustments | `time_off.manage`; step-up | member\_id, policy\_id, amount\_seconds, effective\_on, note | 201 ledger row | note required | key required |
| GET | /v1/time-off/requests | own or `time_off.approve` in scope | status, from, to | requests | — | natural |
| POST | /v1/time-off/requests | own (or manager on behalf) | policy\_id, starts\_on, ends\_on, day\_parts?, reason | 201 (auto-approved if policy allows) | 422 `insufficient_balance` unless negative allowed; 409 overlapping request | key required |
| POST | /v1/time-off/requests/{id}/approve, /deny, /cancel | approver / requester | note | 200; ledger updated | 409 state; cannot approve own | natural |
| GET/POST | /v1/time-off/holiday-calendars | `time_off.view` / `time_off.manage` | name, country\_code | calendars incl. global presets | — | natural |
| POST/DELETE | /v1/time-off/holiday-calendars/{id}/holidays(/{hid}) | `time_off.manage` | date, name, paid\_seconds, recurring\_yearly | holiday | presets read-only | natural |
| PUT | /v1/time-off/holiday-calendars/{id}/assignments | `time_off.manage` | assignments\[\] {scope\_type, scope\_value} | 200 | — | natural |

## /expenses (+ payroll, invoices)

| Method | Path | Permission | Request | Response | Validation · errors | Idempotency |
| --- | --- | --- | --- | --- | --- | --- |
| POST | /v1/expenses/receipts/upload-url | own | file\_name, mime\_type, bytes | presigned PUT (5 min) + receipt\_id | JPEG, PNG, PDF; ≤ 10 MB | key optional |
| GET | /v1/expenses | own, or `expenses.view` in scope | filter\[member\_id, project\_id, category\_id, status, billable\], from, to | expenses | — | natural |
| POST | /v1/expenses | own; `expenses.create` | project\_id, category\_id, spent\_on, amount, description, billable, receipt\_ids\[\] | 201 draft | amount > 0; project membership; receipt required above category threshold | key required |
| GET/PATCH/DELETE | /v1/expenses/{id} | owner of expense (draft/submitted) or `expenses.manage` | fields | expense | 409 once approved | natural |
| POST | /v1/expenses/{id}/submit | own | — | 200 | 422 missing receipt | natural |
| POST | /v1/expenses/{id}/approve, /reject | `expenses.approve` in scope | note | 200 | cannot approve own; 409 state | natural |
| GET/POST/PATCH/DELETE | /v1/expenses/categories(/{id}) | `expenses.manage` | name, gl\_code, requires\_receipt\_above | categories | unique name | natural |
| GET | /v1/payroll/amounts-owed | `payroll.view` | period\_start, period\_end, member\_ids? | per member: hours, overtime, PTO, holiday, expenses, gross | — | natural |
| GET | /v1/payroll/runs | `payroll.view` | status | runs | — | natural |
| POST | /v1/payroll/runs | `payroll.manage` | period\_start, period\_end, pay\_schedule, member\_ids? | 201 draft run with calculated payments | 409 overlapping run; only approved timesheets (unless org allows unapproved) | key required |
| GET | /v1/payroll/runs/{id} | `payroll.view` | include=payments,line\_items | run | — | natural |
| POST | /v1/payroll/runs/{id}/approve | `payroll.manage`; step-up | — | 202; payments queued to providers | 409 state; 422 member without payout account | key required |
| POST | /v1/payroll/runs/{id}/cancel | `payroll.manage` | — | 200 | only draft or pending | natural |
| GET | /v1/payroll/payments | own or `payroll.view` | member\_id, status, from, to | payments | — | natural |
| POST | /v1/payroll/payments | `payroll.manage`; step-up | member\_id, kind (bonus, reimbursement, adjustment), amount, note, send\_now | 201 | — | key required |
| POST | /v1/payroll/payments/{id}/mark-paid | `payroll.manage` | paid\_at, reference | 200 | only unpaid | natural |
| GET | /v1/payroll/payments/{id}/payslip | own or `payroll.view` | — | presigned PDF URL | — | natural |
| GET/POST/DELETE | /v1/payroll/payout-accounts(/{id}) | own (or `payroll.manage`) | provider, external\_ref | accounts (reference masked) | provider verification | key optional |
| GET/POST/PATCH | /v1/payroll/overtime-policies(/{id}) | `payroll.manage` | thresholds, multipliers, effective\_from, members/teams | policies | effective\_from ≥ next work week | natural |
| GET | /v1/invoices | `invoices.view` (team invoices: own) | filter\[kind, client\_id, status\], from, to | invoices | — | natural |
| POST | /v1/invoices | `invoices.manage` (client) or own (team) | kind, client\_id or —, period, project\_ids, member\_ids, include\_expenses, include\_non\_billable, due\_on, tax\_rate, notes | 201 draft with generated lines | no billable time → 422 `nothing_to_invoice` | key required |
| GET/PATCH/DELETE | /v1/invoices/{id} | same | lines, notes, dates (draft only) | invoice | 409 not draft | natural |
| POST | /v1/invoices/{id}/send | same | recipients\[\], message | 200; time locked; number assigned | ≤ 20 recipients | key required |
| POST | /v1/invoices/{id}/void | same; step-up | reason | 200; time unlocked | 409 if paid | natural |
| POST | /v1/invoices/{id}/payments | same | amount, paid\_on, method, reference | 201 | amount ≤ outstanding | key required |
| GET | /v1/invoices/{id}/pdf | same | — | presigned PDF URL | — | natural |
| GET/POST/PATCH/DELETE | /v1/invoices/recurring(/{id}) | `invoices.manage` | client\_id, cadence, template, auto\_send | schedules | — | natural |

## /reports

Every report type is backed by one typed query class, so the UI, exports, schedules, the public API and the AI layer all get identical numbers and permission checks.

| Method | Path | Permission | Request | Response | Validation · errors | Idempotency |
| --- | --- | --- | --- | --- | --- | --- |
| GET | /v1/reports/types | any member | — | report types available to the caller, with filters, columns and group-bys | — | natural |
| POST | /v1/reports/query | per report type (e.g. `reports.time`, `reports.payroll`) + data scope | type, filters, columns, group\_by, from, to, timezone?, cursor | rows + totals (synchronous) | 422 unknown column/filter; > 10,000 rows or > 20 s → 409 `use_export` | natural (read) |
| POST | /v1/reports/exports | same | same as query + format (csv, xlsx, pdf) | 202 `{export_id}` | 429 above 10 exports/hour/member | key required |
| GET | /v1/reports/exports/{id} | requester or `reports.manage` | — | status, row\_count, `download_url` (presigned, 5 min) when ready | 410 `expired` after 7 days | natural |
| GET | /v1/reports/dashboard | any member | widgets\[\]?, team\_id? | widget data (own or scoped) | — | natural |
| GET/POST/PATCH/DELETE | /v1/reports/saved(/{id}) | owner of the saved report; org-visible needs `reports.manage` | type, name, filters, columns, group\_by, visibility, favorite | saved reports | — | natural / key optional |
| GET/POST/PATCH/DELETE | /v1/reports/schedules(/{id}) | owner of the saved report | saved\_report\_id, cadence, day, local\_time, timezone, format, recipients | schedules | external emails only if org allows | natural / key optional |
| POST | /v1/reports/schedules/{id}/run-now, /pause, /resume | same | — | 202 / 200 | — | key required (run-now) |
| GET | /v1/audit-logs | `audit.view` | filter\[actor\_member\_id, action, target\_type, target\_id\], from, to | audit entries | range ≤ 366 days | natural |
| POST | /v1/audit-logs/exports | `audit.view`; step-up | filters, format | 202 export | — | key required |
| GET | /v1/jobs/{id} | requester | — | status of any async job (imports, exports, syncs), progress, result link | — | natural |

## /billing

Available while the org is suspended, so owners can pay and restore access.

| Method | Path | Permission | Request | Response | Validation · errors | Idempotency |
| --- | --- | --- | --- | --- | --- | --- |
| GET | /v1/billing/plans | any member | currency? | public plans, add-ons, prices | — | natural |
| GET | /v1/billing/subscription | `billing.view` | — | plan, interval, status, seats purchased/used, trial end, period, pending change, add-ons | — | natural |
| GET | /v1/billing/entitlements | any member | — | resolved features and limits | — | natural |
| GET | /v1/billing/usage | `billing.view` | period? | metered usage vs limits (screenshots, app/URL records, AI tokens) | — | natural |
| POST | /v1/billing/checkout | `billing.manage` | plan\_code, interval, seats, addons\[\] | provider checkout URL or session | seats ≥ active billable members; plan exists | key required |
| GET | /v1/billing/subscription/preview | `billing.manage` | plan\_code?, interval?, seats?, addons? | proration preview: amount now, next invoice | — | natural |
| POST | /v1/billing/subscription/change | `billing.manage`; step-up | plan\_code?, interval?, addons? | 200; upgrades immediate, downgrades scheduled | 409 downgrade would exceed new plan limits (lists conflicts) | key required |
| POST | /v1/billing/subscription/cancel | owner; step-up | reason, feedback | 200 `cancel_at` period end | — | key required |
| POST | /v1/billing/subscription/resume | owner | — | 200 | only before `cancel_at` | natural |
| GET/PATCH | /v1/billing/customer | `billing.manage` | billing\_email, company\_name, address, tax\_id | customer | tax ID format per country | natural |
| POST | /v1/billing/portal-session | `billing.manage` | return\_url | provider portal URL (payment methods) | return\_url must be the app origin | natural |
| GET | /v1/billing/invoices | `billing.view` | — | invoices with hosted/PDF links | — | natural |
| POST | /hooks/billing/{provider} | provider signature | provider event | 200 | 401 bad signature; duplicates acknowledged, not reprocessed | natural (provider event ID) |

## /integrations, /webhooks

| Method | Path | Permission | Request | Response | Validation · errors | Idempotency |
| --- | --- | --- | --- | --- | --- | --- |
| GET | /v1/integrations | `integrations.view` | — | providers with capabilities, plan availability and this org's connections | — | natural |
| POST | /v1/integrations/{provider}/connect | `integrations.manage` | return\_url, api\_key? (key-based providers) | authorization URL with signed `state`, or 201 connection | 403 `entitlement_required` (integration count) | key optional |
| GET | /v1/integrations/oauth/callback | signed state | code, state | 302 to the app with connection ID | state tied to org, member and expiry; 401 on mismatch | natural (code single-use) |
| GET | /v1/integrations/connections/{id} | `integrations.view` | — | status, settings, last sync, errors | — | natural |
| PATCH | /v1/integrations/connections/{id} | `integrations.manage` | settings {write\_back\_mode, import\_options} | 200 | — | natural |
| DELETE | /v1/integrations/connections/{id} | `integrations.manage` | keep\_tasks? | 204; tokens revoked at provider | — | natural |
| GET | /v1/integrations/connections/{id}/external-projects | `integrations.manage` | q, cursor | projects in the external tool | 424 `provider_error` with provider message | natural |
| PUT | /v1/integrations/connections/{id}/project-links | `integrations.manage` | links\[\] {external\_project\_id, project\_id or create\_new} | 200; initial import queued | one link per project | natural |
| PUT | /v1/integrations/connections/{id}/user-links | `integrations.manage` | links\[\] {external\_user\_id, member\_id} | 200 | — | natural |
| POST | /v1/integrations/connections/{id}/sync | `integrations.manage` | direction? | 202 job | 429 one manual sync per 5 min | key optional |
| GET | /v1/integrations/connections/{id}/sync-runs | `integrations.view` | — | recent runs with stats and errors | — | natural |
| POST | /hooks/integrations/{provider} | provider signature | provider payload | 200 | signature check; stored and processed async | natural (external event ID) |
| GET | /v1/webhooks/event-types | `webhooks.manage` | — | event catalog with payload schemas | — | natural |
| GET/POST | /v1/webhooks | `webhooks.manage` | url, events\[\], description | 201 endpoint + signing secret (shown once); verification ping sent | HTTPS only; private IPs rejected; ≤ 20 endpoints per org | key optional |
| GET/PATCH/DELETE | /v1/webhooks/{id} | `webhooks.manage` | url, events, status (enable/disable) | endpoint | URL change re-verifies | natural |
| POST | /v1/webhooks/{id}/rotate-secret | `webhooks.manage`; step-up | — | new secret; old valid 24 h | — | key required |
| POST | /v1/webhooks/{id}/test | `webhooks.manage` | event\_type | delivery result | — | natural |
| GET | /v1/webhooks/{id}/deliveries | `webhooks.manage` | status, from | deliveries (30 days) | — | natural |
| POST | /v1/webhooks/deliveries/{id}/replay | `webhooks.manage` | — | 202 | — | key optional |

**Webhook events (v1):** `timer.started`, `timer.stopped`, `time_entry.created`, `time_entry.updated`, `time_entry.deleted`, `timesheet.submitted`, `timesheet.approved`, `timesheet.denied`, `manual_time_request.created`, `project.created`, `project.updated`, `project.archived`, `task.created`, `task.updated`, `task.completed`, `client.created`, `member.joined`, `member.removed`, `shift.late`, `shift.missed`, `shift.abandoned`, `break.started`, `break.ended`, `time_off_request.created`, `time_off_request.approved`, `expense.submitted`, `expense.approved`, `payment.sent`, `invoice.sent`, `invoice.paid`, `budget.threshold_reached`, `anomaly.detected`, `billing.seats_changed`. Payload: `{id, type, occurred_at, organization_id, data}`; header `Worklume-Signature: t=…,v1=…`.

## /devices, /desktop

`/v1/desktop/*` is the agent protocol, served by the Ingest service. It needs a DPoP-bound device token whose device is active and belongs to the token's member. The mobile app uses the same sync, heartbeat and config endpoints.

| Method | Path | Permission | Request | Response | Validation · errors | Idempotency |
| --- | --- | --- | --- | --- | --- | --- |
| GET | /v1/devices | own, or `devices.manage` all | filter\[member\_id, platform, status\] | devices with capabilities, version, last seen | — | natural |
| GET | /v1/devices/{id} | own or `devices.manage` | — | device, capability map, config version, queue depth | — | natural |
| PATCH | /v1/devices/{id} | own | name | 200 | — | natural |
| DELETE | /v1/devices/{id} | own or `devices.manage` | — | 204; device revoked, its sessions revoked, `sign_out` command queued | — | natural |
| POST | /v1/devices/{id}/commands | `devices.manage` | command (refresh\_config, force\_sync, upload\_logs, stop\_timer), payload | 202 command | 409 device revoked | key optional |
| POST | /v1/desktop/register | device token (first use) | install\_id, platform, os\_version, app\_version, public\_key (JWK), capabilities, hardware\_hash | 200 `{device_id, config}` | 403 `platform_not_allowed` by policy; 426 `upgrade_required` below minimum version | natural (install\_id) |
| GET | /v1/desktop/config | device | `If-None-Match: <config_version>` | 200 policies (tracking, screenshots, blur, app/URL level, idle, limits, auto-start, breaks, privacy notice version), entitlements; or 304 | — | natural |
| GET | /v1/desktop/projects | device | since? (delta cursor) | trackable projects and tasks for this member, with deletions | — | natural |
| POST | /v1/desktop/notice/acknowledge | device | notice\_id | 204; tracking allowed | 409 not latest version | natural |
| POST | /v1/desktop/sync | device | batch\_id, first\_seq, last\_seq, events\[\] (≤ 500, ≤ 5 MB) | 200 `{applied_seq, results: [{event_id, status: accepted, duplicate, conflict, rejected, code, details}], commands[]}` | 413; 422 invalid envelope (whole batch); per-event codes: `time_locked`, `limit_reached`, `project_unavailable`, `overlap_trimmed`, `clock_adjusted` | key = batch\_id; per event = event\_id |
| POST | /v1/desktop/screenshots/upload-urls | device | items\[\] (≤ 20) {id, captured\_at, slot\_start, monitor\_index, bytes, sha256} | per item: presigned PUT URL + headers, or `rejected` with code | `quota_exceeded`, `slot_count_exceeded`, `screenshots_disabled` | client ID (screenshot id) |
| POST | /v1/desktop/heartbeat | device | timer\_state, queue\_depth, app\_version, clock (device UTC), battery? | `{server_time, config_version, pending_commands}` | Redis only; 1 per 60 s max | natural |
| POST | /v1/desktop/commands/{id}/ack | device | status, result? | 204 | — | natural |
| PUT | /v1/desktop/capabilities | device | capability map | 204; dashboard updated; admins alerted on newly denied capabilities | — | natural |
| GET | /v1/desktop/updates | device | platform, channel, version | latest allowed release (respects rollout %), signature, URL; or 204 | — | natural |
| POST | /v1/desktop/logs/upload-url | device (only after an `upload_logs` command) | command\_id, bytes | presigned PUT | ≤ 20 MB | natural |

## /ai

All AI endpoints return 403 `ai_disabled` when the org has switched AI off, and respect the same data scope as reports: nobody sees AI output about members they could not see directly.

| Method | Path | Permission | Request | Response | Validation · errors | Idempotency |
| --- | --- | --- | --- | --- | --- | --- |
| GET | /v1/ai/settings | `ai.view` | — | enabled, allowed data tiers, audiences, quotas | — | natural |
| PUT | /v1/ai/settings | owner | enabled, allow\_labels (tier 1), summaries\_for (self, managers), anomaly\_rules\[\] | 200; privacy notice version bumped if needed | 403 `entitlement_required` | natural |
| GET | /v1/ai/insights | own, or `ai.view` in scope | filter\[insight\_type, subject\_type, subject\_id, status\], from, to | insights with evidence links | — | natural |
| GET | /v1/ai/insights/{id} | same | — | insight, evidence, model version | — | natural |
| PATCH | /v1/ai/insights/{id} | recipient | status (seen, acknowledged, dismissed, actioned) | 200 | — | natural |
| POST | /v1/ai/insights/{id}/feedback | recipient | rating (-1, 0, 1), comment? | 204 | — | natural |
| GET | /v1/ai/anomalies | `anomalies.review` in scope | filter\[status, member\_id, rule\_key, confidence\], from, to | anomalies | — | natural |
| GET | /v1/ai/anomalies/{id} | same (or the subject member) | — | anomaly with evidence and explanation | — | natural |
| POST | /v1/ai/anomalies/{id}/review | `anomalies.review` | decision (confirmed, dismissed), note | 200 | 409 already reviewed | natural |
| POST | /v1/ai/queries | `ai.query` | question (≤ 500 chars), context? {team\_id, project\_id, from, to}; `Accept: text/event-stream` for streaming | 200 answer, executed plan, citations (report links) | 422 `question_out_of_scope` (refused with reason); 429 `quota_exceeded` | key optional |
| GET | /v1/ai/queries | own | — | own question history (90 days) | — | natural |
| POST | /v1/ai/queries/{id}/feedback | own | rating, comment? | 204 | — | natural |
| POST | /v1/ai/classification-suggestions | `classifications.manage` | scope (job\_type\_id?), limit | 202 job; suggestions appear as `source = ai_suggested` for approval | — | key required |
| GET | /v1/ai/usage | `billing.view` | period | tokens and cost by capability vs quota | — | natural |

**Internal only** (AI plane → core, mTLS + service token, never public): `POST /internal/ai/insights`, `POST /internal/ai/anomalies`, `POST /internal/ai/classification-suggestions`. Each call carries `organization_id`, is validated against the org's AI settings and tier rules, and is written through the owning domain service.

**Endpoint count:** more than 250 operations across 13 groups, plus OAuth, SCIM and inbound webhook endpoints.
