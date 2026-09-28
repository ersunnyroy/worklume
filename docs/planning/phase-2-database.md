# Database — complete table catalog

PostgreSQL 17+. 142 tables in 16 sections. Every tenant-owned table carries `organization_id` and is protected by composite foreign keys, row-level security and application scopes (Architecture §4).

## 0. Conventions shared by every table

Each table below lists only its own columns. The standard columns in this section are implied by the table's header line.

| Header tag | Implied columns and constraints |
| --- | --- |
| every table | `id uuid PRIMARY KEY` (UUIDv7) unless another key is shown; `created_at timestamptz NOT NULL DEFAULT now()`; `updated_at timestamptz NOT NULL DEFAULT now()` (omitted on append-only tables) |
| **Tenant** | `organization_id uuid NOT NULL REFERENCES organizations(id)`; `UNIQUE (organization_id, id)`; RLS policy on `organization_id`; every FK to another tenant table is composite `(organization_id, x_id)` |
| **Soft delete** | `deleted_at timestamptz NULL`; uniques and indexes carry `WHERE deleted_at IS NULL` |
| **Audit A / E / N** | A = changes written to `audit_logs`; E = A plus a domain edit-history table; N = no per-row audit (telemetry) |
| **Partitioned** | Monthly range partitions on the named column; PK becomes `(id, <column>)`; no inbound FKs |
| **Global** | No `organization_id`; catalog or identity data with no tenant content |

FK notation: `→ table` means a composite FK `(organization_id, col) → table(organization_id, id)` for tenant tables, or a plain FK for global tables. `ON DELETE` is `RESTRICT` unless stated.

## 1. Identity & authentication

### `users`

One login identity per person, shared across all organizations they belong to. **Global · Soft delete · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| email | citext | NOT NULL |
| email\_verified\_at | timestamptz | NULL |
| name | text | NOT NULL, 1–120 chars |
| password\_hash | text | NULL (SSO-only or passkey-only users); Argon2id |
| locale | text | DEFAULT 'en' |
| timezone | text | IANA name; personal default |
| avatar\_key | text | S3 key, NULL |
| status | text | CHECK in (active, locked, disabled) |
| mfa\_required | boolean | DEFAULT false (set true if any org enforces MFA) |
| last\_login\_at | timestamptz | NULL |

Unique: `lower(email)` WHERE deleted\_at IS NULL. Indexes: status.

### `user_mfa_factors`

TOTP and email-code factors. **Global · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| user\_id | uuid | FK → users ON DELETE CASCADE |
| type | text | CHECK in (totp, email) |
| secret\_encrypted | bytea | TOTP seed, envelope-encrypted with KMS |
| label | text |  |
| confirmed\_at | timestamptz | NULL until first valid code |
| last\_used\_at | timestamptz |  |
| last\_used\_step | bigint | TOTP replay protection |

Indexes: (user\_id).

### `webauthn_credentials`

Passkeys and security keys. **Global · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| user\_id | uuid | FK → users ON DELETE CASCADE |
| credential\_id | bytea | NOT NULL |
| public\_key | bytea | COSE key |
| sign\_count | bigint | clone detection |
| aaguid | uuid | authenticator model |
| transports | text\[\] |  |
| backup\_eligible | boolean | synced passkey |
| name | text | user label |
| last\_used\_at | timestamptz |  |

Unique: credential\_id. Indexes: (user\_id).

### `mfa_recovery_codes`

Ten single-use recovery codes. **Global · Audit A · append-only**

| Column | Type | Notes |
| --- | --- | --- |
| user\_id | uuid | FK → users ON DELETE CASCADE |
| code\_hash | text | Argon2id |
| used\_at | timestamptz | NULL |

Indexes: (user\_id) WHERE used\_at IS NULL.

### `user_sessions`

One row per signed-in client (browser, desktop, mobile); the anchor for refresh tokens and remote sign-out. **Global · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| user\_id | uuid | FK → users ON DELETE CASCADE |
| organization\_id | uuid | FK → organizations; the org the session's tokens are bound to; changes on org switch |
| member\_id | uuid | FK → organization\_members, NULL until an org is chosen |
| client\_type | text | CHECK in (web, desktop, mobile, api) |
| device\_id | uuid | FK → devices, NULL for web |
| auth\_method | text | password, passkey, sso, refresh |
| mfa\_at | timestamptz | step-up freshness |
| ip | inet | last seen |
| user\_agent | text |  |
| last\_seen\_at | timestamptz |  |
| expires\_at | timestamptz | absolute limit |
| revoked\_at | timestamptz | NULL |
| revoke\_reason | text |  |

Indexes: (user\_id) WHERE revoked\_at IS NULL; (organization\_id, member\_id); (device\_id).

### `refresh_tokens`

Rotating refresh tokens with reuse detection. **Global (org-bound through session) · Audit N**

| Column | Type | Notes |
| --- | --- | --- |
| session\_id | uuid | FK → user\_sessions ON DELETE CASCADE |
| family\_id | uuid | all rotations of one login |
| token\_hash | bytea | SHA-256 of the opaque token |
| oauth\_client\_id | uuid | FK → oauth\_clients, NULL for first-party |
| scopes | text\[\] |  |
| dpop\_jkt | text | key thumbprint when bound (desktop, mobile) |
| expires\_at | timestamptz | sliding: 30 days web/mobile, 90 days desktop |
| used\_at | timestamptz | set on rotation; reuse of a used token revokes the whole family |
| revoked\_at | timestamptz |  |

Unique: token\_hash. Indexes: (family\_id); (session\_id).

### `personal_access_tokens`

User-created API tokens, bound to one organization. **Tenant · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| member\_id | uuid | → organization\_members |
| name | text |  |
| token\_prefix | text | first 8 chars, shown in UI (`wlm_pat_…`) |
| token\_hash | bytea | SHA-256 |
| scopes | text\[\] | subset of the member's permissions |
| last\_used\_at | timestamptz |  |
| last\_used\_ip | inet |  |
| expires\_at | timestamptz | max 1 year |
| revoked\_at | timestamptz |  |

Unique: token\_hash. Indexes: (organization\_id, member\_id).

### `organization_access_tokens`

Org-level service tokens for automation, not tied to a person. **Tenant · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| name | text |  |
| token\_prefix | text | `wlm_oat_…` |
| token\_hash | bytea |  |
| scopes | text\[\] |  |
| created\_by\_member\_id | uuid | → organization\_members |
| last\_used\_at | timestamptz |  |
| expires\_at | timestamptz | NULL = no expiry (requires owner) |
| revoked\_at | timestamptz |  |

Unique: token\_hash.

### `oauth_clients`

Third-party and first-party OAuth applications. **Global · Soft delete · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| owner\_organization\_id | uuid | FK → organizations, NULL for Worklume-owned apps |
| name | text |  |
| client\_id | text | public identifier |
| client\_secret\_hash | text | NULL for public clients (PKCE only) |
| redirect\_uris | text\[\] | exact match |
| allowed\_scopes | text\[\] |  |
| is\_first\_party | boolean | web, desktop, mobile |
| is\_confidential | boolean |  |
| logo\_key | text |  |

Unique: client\_id.

### `oauth_authorizations`

A member's grant of an OAuth app to one organization. **Tenant · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| oauth\_client\_id | uuid | FK → oauth\_clients |
| member\_id | uuid | → organization\_members |
| scopes | text\[\] |  |
| revoked\_at | timestamptz |  |

Unique: (organization\_id, oauth\_client\_id, member\_id) WHERE revoked\_at IS NULL.

### `oauth_authorization_codes`

Short-lived codes for the authorization-code + PKCE flow. **Global · Audit N**

| Column | Type | Notes |
| --- | --- | --- |
| code\_hash | bytea |  |
| oauth\_client\_id | uuid | FK → oauth\_clients |
| user\_id | uuid | FK → users |
| organization\_id | uuid | FK → organizations |
| redirect\_uri | text |  |
| scopes | text\[\] |  |
| code\_challenge | text | S256 only |
| nonce | text |  |
| expires\_at | timestamptz | 60 s |
| used\_at | timestamptz |  |

Unique: code\_hash.

### `one_time_tokens`

Email verification, password reset, magic sign-in, invite acceptance, device linking. **Global · Audit N**

| Column | Type | Notes |
| --- | --- | --- |
| user\_id | uuid | FK → users, NULL for invites to new emails |
| purpose | text | CHECK in (verify\_email, reset\_password, email\_code, device\_link) |
| token\_hash | bytea |  |
| payload | jsonb | e.g. device code |
| attempts | smallint | locked after 5 |
| expires\_at | timestamptz | 5 min–24 h by purpose |
| consumed\_at | timestamptz |  |

Unique: token\_hash. Indexes: (user\_id, purpose).

### `user_identities`

Links a user to an external identity (SAML/OIDC subject). **Global · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| user\_id | uuid | FK → users ON DELETE CASCADE |
| sso\_connection\_id | uuid | FK → sso\_connections |
| subject | text | IdP NameID / `sub` |
| last\_login\_at | timestamptz |  |

Unique: (sso\_connection\_id, subject).

### `auth_events`

Security log of sign-in attempts, MFA challenges and token reuse. **Global · Partitioned by occurred\_at · append-only · retained 1 year**

| Column | Type | Notes |
| --- | --- | --- |
| occurred\_at | timestamptz | partition key |
| user\_id | uuid | NULL if unknown email |
| email\_hash | bytea | for unknown-user throttling |
| organization\_id | uuid | NULL |
| event | text | login\_success, login\_failed, mfa\_failed, token\_reuse, lockout, password\_changed |
| method | text |  |
| ip | inet |  |
| user\_agent | text |  |
| risk\_score | smallint |  |

Indexes: (user\_id, occurred\_at); (ip, occurred\_at).

## 2. Organizations, members, policies, RBAC, teams

### `organizations`

The tenant root. Every tenant row points here. **Global root · Soft delete (archive) · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| name | text | NOT NULL |
| slug | text | internal handle only (no custom domains) |
| status | text | CHECK in (trialing, active, past\_due, suspended, cancelled, archived) |
| timezone | text | IANA; reports and payroll use it |
| currency | char(3) | default for rates and invoices |
| week\_start | smallint | 0–6 |
| locale | text |  |
| country\_code | char(2) | holidays, tax |
| data\_region | text | e.g. `ap-south-1`; fixed at creation |
| owner\_member\_id | uuid | FK → organization\_members (deferrable) |
| trial\_ends\_at | timestamptz |  |
| suspended\_at | timestamptz |  |
| cancelled\_at | timestamptz |  |
| purge\_after | timestamptz | hard-delete date after cancellation |
| settings\_version | integer | bumps when any policy changes; agents poll it |

Unique: slug. Indexes: status; purge\_after WHERE purge\_after IS NOT NULL.

### `organization_members`

A person inside one organization. All tenant data references `member_id`, never `user_id`. **Tenant · Soft delete · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| user\_id | uuid | FK → users |
| status | text | CHECK in (invited, active, suspended, removed) |
| display\_name | text | org-specific name |
| job\_title | text |  |
| employee\_code | text |  |
| job\_type\_id | uuid | → job\_types, NULL |
| employment\_type | text | employee, contractor, other |
| country\_code | char(2) | holiday assignment |
| timezone | text | override; NULL = user's |
| manager\_member\_id | uuid | → organization\_members, NULL |
| tracking\_enabled | boolean | DEFAULT true |
| seat\_billable | boolean | derived from role; Viewers false |
| joined\_at | timestamptz |  |
| removed\_at | timestamptz |  |
| scim\_external\_id | text |  |

Unique: (organization\_id, user\_id); (organization\_id, employee\_code) WHERE employee\_code IS NOT NULL; (organization\_id, scim\_external\_id). Indexes: (organization\_id, status); (organization\_id, manager\_member\_id).

### `invitations`

Pending invites, including the access they will grant. **Tenant · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| email | citext |  |
| role\_id | uuid | → roles (organization-level role) |
| grants | jsonb | teams, projects with project roles, pay rate, pay period |
| token\_hash | bytea |  |
| invited\_by\_member\_id | uuid | → organization\_members |
| expires\_at | timestamptz | 14 days |
| accepted\_member\_id | uuid | → organization\_members |
| accepted\_at | timestamptz |  |
| revoked\_at | timestamptz |  |
| send\_count | smallint | resend throttle |

Unique: token\_hash; (organization\_id, lower(email)) WHERE accepted\_at IS NULL AND revoked\_at IS NULL.

### `job_types`

Roles like "Developer" or "Designer" used for productivity classification and benchmarks. **Tenant · Soft delete · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| name | text |  |
| template\_key | text | NULL or global template (developer, designer, support…) |
| expected\_daily\_seconds | integer | for utilization |

Unique: (organization\_id, lower(name)) WHERE deleted\_at IS NULL.

### `policy_definitions`

Catalog of every configurable policy key. **Global**

| Column | Type | Notes |
| --- | --- | --- |
| key | text | PK, e.g. `screenshots.frequency`, `idle.timeout_seconds` |
| domain | text | owning domain |
| value\_schema | jsonb | JSON Schema for validation |
| default\_value | jsonb |  |
| allowed\_scopes | text\[\] | organization, team, member, project |
| entitlement\_key | text | plan feature required, NULL if none |
| agent\_visible | boolean | shipped to desktop/mobile config |

### `policy_values`

Overrides of policy defaults. Resolution order: member → team → project → organization → default. **Tenant · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| policy\_key | text | FK → policy\_definitions |
| scope\_type | text | CHECK in (organization, team, member, project) |
| scope\_id | uuid | NULL when scope\_type = organization |
| value | jsonb | validated against value\_schema |
| effective\_from | timestamptz | future-dated changes (e.g. overtime next week) |
| updated\_by\_member\_id | uuid | → organization\_members |

Unique: (organization\_id, policy\_key, scope\_type, coalesce(scope\_id, zero-uuid), effective\_from). Indexes: (organization\_id, scope\_type, scope\_id).

### `custom_fields` and `custom_field_values`

Org-defined member fields (department, cost centre). **Tenant · Soft delete · Audit A**

| Table.Column | Type | Notes |
| --- | --- | --- |
| custom\_fields.name | text |  |
| custom\_fields.field\_type | text | text, number, date, select |
| custom\_fields.options | jsonb | for select |
| custom\_fields.required | boolean |  |
| custom\_field\_values.custom\_field\_id | uuid | → custom\_fields |
| custom\_field\_values.member\_id | uuid | → organization\_members |
| custom\_field\_values.value | jsonb |  |

Unique: custom\_field\_values (organization\_id, custom\_field\_id, member\_id).

### `permissions`

Catalog of permission keys, e.g. `time_entries.edit_others`, `screenshots.view`. **Global · PK key (text)**

| Column | Type | Notes |
| --- | --- | --- |
| key | text | PK |
| domain | text |  |
| scope\_levels | text\[\] | levels where it can be granted |
| description | text |  |
| sensitive | boolean | screenshots, pay data: needs explicit grant, never implied |

### `roles`

System roles (Owner, Manager, Member, Viewer, Project Manager, Project User, Project Viewer, Team Lead, IT Admin) and custom roles. **Tenant-or-global · Soft delete · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| organization\_id | uuid | NULL for system roles |
| key | text |  |
| name | text |  |
| scope\_level | text | CHECK in (organization, team, project) |
| is\_system | boolean | system roles cannot be edited |
| description | text |  |

Unique: (coalesce(organization\_id, zero-uuid), key). RLS allows `organization_id IS NULL OR = ctx`.

### `role_permissions`

**Tenant-or-global · Audit A · PK (role\_id, permission\_key)**

| Column | Type | Notes |
| --- | --- | --- |
| role\_id | uuid | FK → roles ON DELETE CASCADE |
| permission\_key | text | FK → permissions |
| organization\_id | uuid | NULL for system roles |

### `role_assignments`

Grants a role to a member at a scope. **Tenant · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| member\_id | uuid | → organization\_members |
| role\_id | uuid | FK → roles |
| scope\_type | text | CHECK in (organization, team, project) |
| scope\_id | uuid | NULL for organization |
| granted\_by\_member\_id | uuid | → organization\_members |
| source | text | manual, invitation, scim |

Unique: (organization\_id, member\_id, role\_id, scope\_type, coalesce(scope\_id, zero-uuid)). Indexes: (organization\_id, scope\_type, scope\_id); CHECK exactly one organization-level role per member (enforced by a partial unique index on (organization\_id, member\_id) WHERE scope\_type = 'organization').

### `teams`

**Tenant · Soft delete · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| name | text |  |
| description | text |  |
| scim\_external\_id | text | NULL; SCIM-managed teams are read-only in UI |

Unique: (organization\_id, lower(name)) WHERE deleted\_at IS NULL; (organization\_id, scim\_external\_id).

### `team_members`

**Tenant · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| team\_id | uuid | → teams ON DELETE CASCADE |
| member\_id | uuid | → organization\_members |

Unique: (organization\_id, team\_id, member\_id). Indexes: (organization\_id, member\_id). Team leads are `role_assignments` with scope\_type = team.

## 3. Clients, projects, tasks, rates, budgets

### `clients`

**Tenant · Soft delete · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| name | text |  |
| email | citext | invoice recipient |
| phone | text |  |
| address | jsonb | line1, line2, city, region, postcode, country |
| tax\_id | text |  |
| currency | char(3) | NULL = org currency |
| payment\_terms\_days | smallint |  |
| invoice\_notes | text |  |
| archived\_at | timestamptz | archived clients hidden from pickers |

Unique: (organization\_id, lower(name)) WHERE deleted\_at IS NULL.

### `projects`

**Tenant · Soft delete · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| client\_id | uuid | → clients, NULL |
| name | text |  |
| code | text | short code, NULL |
| description | text |  |
| color | text | hex |
| billable\_default | boolean | new time is billable unless changed |
| status | text | CHECK in (active, archived) |
| start\_on / end\_on | date | NULL |
| record\_activity | boolean | FALSE = no activity % for this project |
| created\_by\_member\_id | uuid | → organization\_members |
| archived\_at | timestamptz |  |

Unique: (organization\_id, lower(name)) WHERE deleted\_at IS NULL; (organization\_id, code) WHERE code IS NOT NULL. Indexes: (organization\_id, client\_id); (organization\_id, status).

### `project_members`

Who may track on or manage a project. **Tenant · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| project\_id | uuid | → projects ON DELETE CASCADE |
| member\_id | uuid | → organization\_members |
| project\_role | text | CHECK in (manager, user, viewer) |
| added\_via\_team\_id | uuid | → teams, NULL (membership through a team) |

Unique: (organization\_id, project\_id, member\_id). Indexes: (organization\_id, member\_id). Project-role permissions come from the matching system roles.

### `tasks`

To-dos under a project, native or imported from an integration. **Tenant · Soft delete · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| project\_id | uuid | → projects |
| parent\_task\_id | uuid | → tasks, NULL (subtasks) |
| title | text | 1–500 chars |
| description | text |  |
| status | text | CHECK in (open, in\_progress, done, archived) |
| due\_on | date |  |
| estimate\_seconds | integer |  |
| position | numeric | ordering |
| source | text | native or integration provider key |
| completed\_at | timestamptz |  |
| created\_by\_member\_id | uuid | → organization\_members |

Indexes: (organization\_id, project\_id, status); (organization\_id, parent\_task\_id). External IDs live in `integration_entity_maps`.

### `task_assignees`

**Tenant · Audit A · PK (organization\_id, task\_id, member\_id)**

| Column | Type | Notes |
| --- | --- | --- |
| task\_id | uuid | → tasks ON DELETE CASCADE |
| member\_id | uuid | → organization\_members |

Indexes: (organization\_id, member\_id).

### `rates`

Pay and bill rates with effective dates, per member and optionally per project. **Tenant · Audit E (append new rows; never edit history)**

| Column | Type | Notes |
| --- | --- | --- |
| member\_id | uuid | → organization\_members |
| project\_id | uuid | → projects, NULL = default for member |
| rate\_type | text | CHECK in (pay, bill) |
| unit | text | CHECK in (hourly, fixed\_weekly, fixed\_monthly) |
| amount\_minor | bigint | ≥ 0 |
| currency | char(3) |  |
| effective\_from | date |  |
| effective\_to | date | NULL = open |
| created\_by\_member\_id | uuid |  |

Constraint: `EXCLUDE USING gist (organization_id WITH =, member_id WITH =, coalesce(project_id, zero-uuid) WITH =, rate_type WITH =, daterange(effective_from, effective_to) WITH &&)` — no overlapping periods. Indexes: (organization\_id, member\_id, rate\_type, effective\_from).

### `budgets`

Project or client budgets in hours or money. **Tenant · Soft delete · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| subject\_type | text | CHECK in (project, client) |
| project\_id | uuid | → projects, set when subject\_type = project |
| client\_id | uuid | → clients, set when subject\_type = client |
| basis | text | CHECK in (hours, cost) |
| rate\_basis | text | CHECK in (pay, bill); for cost budgets |
| amount | bigint | seconds or minor units |
| currency | char(3) | cost budgets |
| period | text | CHECK in (total, weekly, monthly) |
| period\_anchor | date |  |
| include\_non\_billable | boolean |  |
| alert\_thresholds | smallint\[\] | e.g. {50, 80, 100} |
| alert\_recipients | text | managers or all members |
| stop\_timers\_at\_pct | smallint | NULL = never |

CHECK exactly one of project\_id / client\_id matches subject\_type. Unique: one active budget per subject.

### `budget_alert_events`

Records each threshold crossing so alerts fire once per period. **Tenant · append-only · Audit N**

| Column | Type | Notes |
| --- | --- | --- |
| budget\_id | uuid | → budgets |
| period\_start | date |  |
| threshold\_pct | smallint |  |
| consumed | bigint | at crossing |
| action | text | alerted, timers\_stopped |

Unique: (organization\_id, budget\_id, period\_start, threshold\_pct).

## 4. Time tracking

### `timers`

The live timer state per member; one running timer at most. Stopped rows are kept 30 days for sync reconciliation. **Tenant · Audit N (changes produce time entries, which are audited)**

| Column | Type | Notes |
| --- | --- | --- |
| member\_id | uuid | → organization\_members |
| project\_id | uuid | → projects |
| task\_id | uuid | → tasks, NULL |
| device\_id | uuid | → devices; the device that owns the timer |
| source | text | desktop, web, mobile, extension, automatic |
| started\_at | timestamptz | server-corrected start |
| client\_started\_at | timestamptz | device clock, for diagnostics |
| stopped\_at | timestamptz | NULL while running |
| stop\_reason | text | user, idle\_discard, limit\_reached, budget\_stop, device\_switch, server |
| current\_time\_entry\_id | uuid | → time\_entries (the open entry) |
| version | integer | optimistic lock |

Unique: (organization\_id, member\_id) WHERE stopped\_at IS NULL. Indexes: (organization\_id, device\_id).

### `time_entries`

The source of truth for tracked time: one row per continuous interval on one project/task. **Tenant · Soft delete · Audit E**

| Column | Type | Notes |
| --- | --- | --- |
| member\_id | uuid | → organization\_members |
| project\_id | uuid | → projects |
| task\_id | uuid | → tasks, NULL |
| started\_at | timestamptz |  |
| ended\_at | timestamptz | NULL while the timer runs |
| duration\_seconds | integer | GENERATED from ended\_at − started\_at |
| source | text | desktop, web, mobile, extension, manual, automatic, integration |
| billable | boolean |  |
| note | text |  |
| device\_id | uuid | → devices, NULL for manual |
| client\_event\_id | uuid | idempotency key from the device |
| manual\_reason | text | required when source = manual |
| timesheet\_id | uuid | → timesheets |
| locked\_at | timestamptz |  |
| lock\_reason | text | timesheet, payment, invoice, export, integration |
| created\_by\_member\_id | uuid | differs from member\_id when a manager adds time |
| version | integer |  |

Constraints: CHECK ended\_at > started\_at; `EXCLUDE USING gist (organization_id WITH =, member_id WITH =, tstzrange(started_at, coalesce(ended_at, 'infinity')) WITH &&) WHERE (deleted_at IS NULL)` — no overlapping time for a member. Unique: (organization\_id, client\_event\_id) WHERE client\_event\_id IS NOT NULL. Indexes: (organization\_id, member\_id, started\_at); (organization\_id, project\_id, started\_at); (organization\_id, timesheet\_id).

### `time_entry_edits`

Full edit history of every time entry. **Tenant · append-only**

| Column | Type | Notes |
| --- | --- | --- |
| time\_entry\_id | uuid | → time\_entries |
| actor\_member\_id | uuid | → organization\_members, NULL = system |
| action | text | create, update, split, delete, restore, lock, unlock |
| before | jsonb |  |
| after | jsonb |  |
| reason | text |  |
| request\_id | uuid |  |

Indexes: (organization\_id, time\_entry\_id, created\_at); (organization\_id, actor\_member\_id, created\_at).

### `manual_time_requests`

Requests to add time that need approval. **Tenant · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| member\_id | uuid | → organization\_members |
| project\_id | uuid | → projects |
| task\_id | uuid | → tasks |
| started\_at / ended\_at | timestamptz |  |
| billable | boolean |  |
| reason | text | NOT NULL |
| status | text | CHECK in (pending, approved, denied, cancelled) |
| reviewed\_by\_member\_id | uuid |  |
| reviewed\_at | timestamptz |  |
| review\_note | text |  |
| time\_entry\_id | uuid | → time\_entries, set on approval |

Indexes: (organization\_id, status, created\_at); (organization\_id, member\_id).

### `idle_periods`

Detected idle intervals and how they were resolved. **Tenant · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| member\_id | uuid | → organization\_members |
| device\_id | uuid | → devices |
| time\_entry\_id | uuid | → time\_entries |
| client\_event\_id | uuid |  |
| started\_at / ended\_at | timestamptz |  |
| resolution | text | CHECK in (pending, kept, discarded, reassigned, stopped, auto\_kept, auto\_discarded) |
| reassigned\_project\_id | uuid | → projects |
| resolved\_at | timestamptz |  |

Unique: (organization\_id, client\_event\_id). Indexes: (organization\_id, member\_id, started\_at).

### `timesheets`

One per member per pay period; the unit of approval and payroll. **Tenant · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| member\_id | uuid | → organization\_members |
| period\_start / period\_end | date | org timezone |
| status | text | CHECK in (open, submitted, approved, denied, paid) |
| submitted\_at | timestamptz |  |
| approved\_at | timestamptz |  |
| denied\_reason | text |  |
| totals | jsonb | snapshot at submit: tracked, manual, billable, overtime, pto, holiday seconds |
| version | integer |  |

Unique: (organization\_id, member\_id, period\_start). Indexes: (organization\_id, status, period\_start).

### `timesheet_approvals`

Each approval step (supports multi-step chains). **Tenant · append-only · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| timesheet\_id | uuid | → timesheets |
| step | smallint | 1 = team lead, 2 = manager… |
| approver\_member\_id | uuid |  |
| decision | text | CHECK in (approved, denied, reopened) |
| note | text |  |

Indexes: (organization\_id, timesheet\_id, step).

## 5. Activity, applications, websites

### `activity_slots`

The core telemetry unit: one row per member, device and 10-minute window. **Tenant · Partitioned by slot\_start · Audit N · Retention: time-log retention (3 years default)**

| Column | Type | Notes |
| --- | --- | --- |
| id | uuid | generated by the agent |
| member\_id | uuid |  |
| device\_id | uuid |  |
| time\_entry\_id | uuid |  |
| project\_id | uuid |  |
| task\_id | uuid | NULL |
| slot\_start | timestamptz | aligned to :00, :10, :20… UTC |
| tracked\_seconds | smallint | 0–600 |
| active\_seconds | smallint | seconds with any input; ≤ tracked |
| keyboard\_seconds | smallint | NULL when the OS gives no split (Wayland, Linux) |
| mouse\_seconds | smallint | NULL when no split |
| idle\_seconds | smallint |  |
| activity\_pct | smallint | GENERATED: active ÷ tracked × 100 |
| input\_method | text | hooks, idle\_poll, portal |
| flags | integer | bitmask: offline\_upload, clock\_adjusted, suspicious\_input, partial\_slot. Plus column input\_features (jsonb): on-device input-timing statistics (e.g. interval entropy) for automated-input detection; never raw events |
| client\_recorded\_at | timestamptz | device clock |
| received\_at | timestamptz | server |

PK (id, slot\_start). Unique: (organization\_id, member\_id, device\_id, slot\_start). Indexes: (organization\_id, member\_id, slot\_start); (organization\_id, project\_id, slot\_start); BRIN (slot\_start).

### `applications`

Global catalog of desktop apps seen across all tenants (names only, no usage). **Global**

| Column | Type | Notes |
| --- | --- | --- |
| platform | text | macos, windows, linux |
| identifier | text | bundle ID, exe name or desktop-file ID, normalized |
| display\_name | text |  |
| publisher | text |  |
| icon\_key | text |  |
| default\_category | text | e.g. development, communication, social, entertainment, ai\_tools |
| suspicious | boolean | known input simulators |

Unique: (platform, identifier).

### `web_domains`

Global catalog of registrable domains. **Global**

| Column | Type | Notes |
| --- | --- | --- |
| domain | text | eTLD+1, lower case |
| display\_name | text |  |
| default\_category | text |  |

Unique: domain.

### `app_usage`

Time in each foreground app, per slot. **Tenant · Partitioned by started\_at · Audit N · Retention: app/URL retention (1 month default)**

| Column | Type | Notes |
| --- | --- | --- |
| id | uuid | agent-generated |
| member\_id | uuid |  |
| device\_id | uuid |  |
| activity\_slot\_id | uuid | soft reference |
| application\_id | uuid | FK → applications (global) |
| window\_title | text | NULL unless the org enables titles; encrypted at rest |
| started\_at | timestamptz |  |
| duration\_seconds | smallint |  |
| project\_id | uuid |  |
| background | boolean | suspicious-app scans |

PK (id, started\_at). Indexes: (organization\_id, member\_id, started\_at); (organization\_id, application\_id, started\_at).

### `url_usage`

Time on each website, per slot. **Tenant · Partitioned by started\_at · Audit N · Retention: app/URL retention**

| Column | Type | Notes |
| --- | --- | --- |
| id | uuid | agent-generated |
| member\_id | uuid |  |
| device\_id | uuid |  |
| activity\_slot\_id | uuid | soft reference |
| web\_domain\_id | uuid | FK → web\_domains |
| url\_path | text | NULL by default (domain-only); full path only if the org enables it; query strings always stripped |
| browser | text |  |
| private\_window | boolean | private windows record domain only or nothing, by policy |
| started\_at | timestamptz |  |
| duration\_seconds | smallint |  |
| project\_id | uuid |  |

PK (id, started\_at). Indexes: (organization\_id, member\_id, started\_at); (organization\_id, web\_domain\_id, started\_at).

### `productivity_classifications`

Org rules that rate an app or domain as core, non-core, unproductive or neutral, optionally per team or job type. **Tenant · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| subject\_type | text | CHECK in (application, web\_domain, category) |
| application\_id | uuid | FK → applications |
| web\_domain\_id | uuid | FK → web\_domains |
| category | text |  |
| scope\_type | text | CHECK in (organization, team, job\_type) |
| scope\_id | uuid |  |
| rating | text | CHECK in (core, non\_core, unproductive, neutral) |
| source | text | CHECK in (template, admin, ai\_suggested) |
| updated\_by\_member\_id | uuid |  |

Unique: (organization\_id, subject\_type, coalesce(application\_id, web\_domain\_id), category, scope\_type, coalesce(scope\_id, zero-uuid)).

## 6. Screenshots and devices

### `screenshots`

Metadata for each captured image; bytes live in S3. **Tenant · Partitioned by captured\_at · Soft delete (tombstone) · Audit A on delete · Retention: screenshot retention (plan: 2 weeks–6 months)**

| Column | Type | Notes |
| --- | --- | --- |
| id | uuid | agent-generated |
| member\_id | uuid |  |
| device\_id | uuid |  |
| activity\_slot\_id | uuid | soft reference |
| time\_entry\_id | uuid | soft reference |
| project\_id | uuid |  |
| captured\_at | timestamptz |  |
| monitor\_index | smallint | 0-based |
| monitor\_count | smallint |  |
| width / height | smallint | after downscale |
| bytes | integer |  |
| content\_sha256 | bytea | integrity check after upload |
| storage\_key | text | server-computed S3 key |
| thumb\_key | text |  |
| kms\_key\_id | text |  |
| blurred | boolean | blur applied on device |
| status | text | CHECK in (pending\_upload, uploaded, processing, ready, failed, deleted, expired) |
| uploaded\_at / processed\_at | timestamptz |  |
| retention\_until | timestamptz | computed at ingest from policy |
| deleted\_by\_member\_id | uuid |  |
| delete\_mode | text | keep\_time, remove\_time |

PK (id, captured\_at). Indexes: (organization\_id, member\_id, captured\_at); (organization\_id, status) WHERE status IN ('pending\_upload', 'processing'); (retention\_until).

### `screenshot_access_log`

Every view or download of a screenshot. **Tenant · Partitioned by occurred\_at · append-only · retained 1 year**

| Column | Type | Notes |
| --- | --- | --- |
| occurred\_at | timestamptz |  |
| screenshot\_id | uuid | soft reference |
| subject\_member\_id | uuid | whose screenshot |
| viewer\_member\_id | uuid |  |
| action | text | view, download, delete |
| ip | inet |  |

Indexes: (organization\_id, subject\_member\_id, occurred\_at); (organization\_id, viewer\_member\_id, occurred\_at).

### `devices`

Each desktop agent, mobile app or browser extension install. **Tenant · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| member\_id | uuid | → organization\_members |
| platform | text | CHECK in (macos, windows, linux\_x11, linux\_wayland, ios, android, chrome\_ext, firefox\_ext) |
| os\_version | text |  |
| app\_version | text |  |
| name | text | user-visible, e.g. "Suni's MacBook" |
| install\_id | uuid | generated at install; stable per install |
| hardware\_hash | bytea | salted hash of machine ID, for duplicate detection |
| public\_key | bytea | device key (Ed25519 or P-256) for DPoP proofs |
| capabilities | jsonb | per feature: granted, denied, unsupported |
| config\_version | integer | last policy version applied |
| mode | text | standard, silent (MDM) |
| last\_seen\_at | timestamptz |  |
| last\_ip | inet |  |
| network\_identity\_hash | bytea | router MAC + public IP hash, office detection |
| status | text | CHECK in (active, revoked, retired) |
| revoked\_at | timestamptz |  |

Unique: (organization\_id, install\_id). Indexes: (organization\_id, member\_id, status).

### `device_heartbeats`

Health samples: agents ping Redis every 60 s; one sample per device is persisted every 5 minutes. **Tenant · Partitioned by received\_at (daily) · retained 30 days · Audit N**

| Column | Type | Notes |
| --- | --- | --- |
| device\_id | uuid |  |
| received\_at | timestamptz |  |
| timer\_state | text |  |
| queue\_depth | integer | unsent events |
| clock\_offset\_ms | integer | device minus server |
| app\_version | text |  |
| battery\_pct | smallint | mobile |

Indexes: (organization\_id, device\_id, received\_at).

### `device_commands`

Server-to-device instructions: refresh config, force sync, sign out, collect logs. **Tenant · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| device\_id | uuid | → devices |
| command | text | refresh\_config, force\_sync, sign\_out, stop\_timer, upload\_logs |
| payload | jsonb |  |
| issued\_by\_member\_id | uuid | NULL = system |
| status | text | pending, delivered, acknowledged, failed, expired |
| expires\_at | timestamptz |  |
| acknowledged\_at | timestamptz |  |

Indexes: (organization\_id, device\_id, status).

### `sync_batches`

Receipt for every upload batch from desktop or mobile, making batch retries idempotent. **Tenant · retained 30 days · Audit N**

| Column | Type | Notes |
| --- | --- | --- |
| device\_id | uuid | → devices |
| batch\_id | uuid | client-generated |
| first\_seq / last\_seq | bigint | device outbox sequence range |
| event\_counts | jsonb | per type: accepted, duplicate, rejected |
| status | text | processing, completed, partial |
| response | jsonb | cached response replayed on retry |

Unique: (organization\_id, device\_id, batch\_id). Indexes: (organization\_id, device\_id, last\_seq).

### `agent_releases`

Desktop builds and staged rollout. **Global · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| platform | text | macos-arm64, macos-x64, windows-x64, linux-x64-deb, linux-x64-rpm |
| channel | text | stable, beta |
| version | text | semver |
| artifact\_url | text |  |
| signature | text | updater signature |
| sha256 | text |  |
| rollout\_pct | smallint | 0–100 |
| min\_supported | boolean | older versions are asked to update |
| released\_at | timestamptz |  |

Unique: (platform, channel, version).

## 7. Attendance, scheduling, breaks, locations

### `shift_series`

A repeating shift pattern. **Tenant · Soft delete · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| member\_id | uuid | → organization\_members |
| rrule | text | RFC 5545 (weekly or bi-weekly with days) |
| local\_start\_time / local\_end\_time | time |  |
| timezone | text | org or member timezone |
| minimum\_seconds | integer | excluding unpaid breaks |
| starts\_on / ends\_on | date |  |
| created\_by\_member\_id | uuid |  |

Indexes: (organization\_id, member\_id).

### `shifts`

Concrete shift instances (materialized 8 weeks ahead from series, or one-off). **Tenant · Soft delete · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| member\_id | uuid |  |
| shift\_series\_id | uuid | → shift\_series, NULL for one-off |
| starts\_at / ends\_at | timestamptz |  |
| timezone | text |  |
| minimum\_seconds | integer |  |
| grace\_seconds | integer | 300–1800 |
| status | text | CHECK in (scheduled, in\_progress, on\_time, early, late, missed, abandoned, cancelled) |
| first\_start\_at | timestamptz | first tracked moment |
| tracked\_seconds | integer | computed |
| evaluated\_at | timestamptz |  |
| is\_exception | boolean | edited occurrence of a series |

Indexes: (organization\_id, member\_id, starts\_at); (organization\_id, status, starts\_at). Unique: (organization\_id, shift\_series\_id, starts\_at).

### `attendance_records`

One row per member per work day: the attendance fact reports read. **Tenant · Audit N (recomputed)**

| Column | Type | Notes |
| --- | --- | --- |
| member\_id | uuid |  |
| work\_date | date | in the member's attendance timezone |
| timezone | text |  |
| shift\_id | uuid | → shifts, NULL |
| first\_start\_at / last\_stop\_at | timestamptz |  |
| tracked\_seconds | integer |  |
| break\_seconds | integer |  |
| status | text | CHECK in (present, late, partial, absent, on\_leave, holiday, non\_working\_day) |
| location\_mode | text | office, remote, field, unknown |
| computed\_at | timestamptz |  |

Unique: (organization\_id, member\_id, work\_date). Indexes: (organization\_id, work\_date, status).

### `break_policies`

**Tenant · Soft delete · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| name | text |  |
| paid | boolean |  |
| max\_duration\_seconds | integer |  |
| rule | text | CHECK in (anytime, once\_per\_session, interval, after\_hours) |
| rule\_params | jsonb | windows or hours |
| auto\_assign\_new\_members | boolean |  |

### `break_policy_members`

**Tenant · Audit A · PK (organization\_id, break\_policy\_id, member\_id)**

| Column | Type | Notes |
| --- | --- | --- |
| break\_policy\_id | uuid | → break\_policies ON DELETE CASCADE |
| member\_id | uuid | → organization\_members |

### `work_breaks`

**Tenant · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| member\_id | uuid |  |
| break\_policy\_id | uuid | → break\_policies |
| device\_id | uuid | → devices |
| client\_event\_id | uuid |  |
| started\_at / ended\_at | timestamptz |  |
| paid | boolean | copied from policy at start |
| abandoned | boolean | exceeded limit, alert sent |

Unique: (organization\_id, client\_event\_id). Indexes: (organization\_id, member\_id, started\_at).

### `job_sites`

Geofenced work locations. **Tenant · Soft delete · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| name | text |  |
| address | text |  |
| center | geography(Point, 4326) |  |
| radius\_m | integer | ≥ 50 |
| project\_id | uuid | → projects, default project on arrival |
| on\_arrival | text | start\_timer, remind, none |
| on\_departure | text | stop\_timer, notify, none |
| restrict\_tracking | boolean | timer only inside the site |

Indexes: GIST (center); (organization\_id).

### `job_site_members`

**Tenant · Audit A · PK (organization\_id, job\_site\_id, member\_id)**

| Column | Type | Notes |
| --- | --- | --- |
| job\_site\_id | uuid | → job\_sites ON DELETE CASCADE |
| member\_id | uuid |  |

### `job_site_visits`

**Tenant · Audit N · Retention: 1 year**

| Column | Type | Notes |
| --- | --- | --- |
| job\_site\_id | uuid | → job\_sites |
| member\_id | uuid |  |
| device\_id | uuid |  |
| client\_event\_id | uuid |  |
| entered\_at / exited\_at | timestamptz |  |
| source | text | geofence, gps\_point, manual |

Unique: (organization\_id, client\_event\_id). Indexes: (organization\_id, job\_site\_id, entered\_at); (organization\_id, member\_id, entered\_at).

### `location_points`

GPS breadcrumbs from mobile. **Tenant · Partitioned by recorded\_at · Audit N · Retention: location retention (1 month default)**

| Column | Type | Notes |
| --- | --- | --- |
| id | uuid | device-generated |
| member\_id | uuid |  |
| device\_id | uuid |  |
| recorded\_at | timestamptz |  |
| position | geography(Point, 4326) |  |
| accuracy\_m | real |  |
| speed\_mps / heading\_deg | real |  |
| battery\_pct | smallint |  |
| tracking\_mode | text | timer, shift, always |
| time\_entry\_id | uuid | soft reference |

PK (id, recorded\_at). Indexes: (organization\_id, member\_id, recorded\_at).

### `office_networks`

Networks treated as "office" for remote-vs-office reporting. **Tenant · Soft delete · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| name | text |  |
| public\_cidr | cidr |  |
| network\_identity\_hash | bytea | router MAC hash |
| auto\_detected | boolean | 3+ members on one network in a day |
| confirmed\_by\_member\_id | uuid |  |

Indexes: (organization\_id, network\_identity\_hash).

## 8. PTO, holidays, overtime

Balances are derived from an append-only ledger, so every hour of leave can be explained and replayed.

### `time_off_policies`

**Tenant · Soft delete · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| name | text |  |
| paid | boolean |  |
| accrual\_type | text | CHECK in (none, annual, monthly, per\_hours\_worked, join\_anniversary) |
| accrual\_amount\_seconds | integer | per period |
| accrual\_per\_worked\_seconds | integer | e.g. 1 h per 80 h worked |
| accrual\_day | smallint | day of month for monthly |
| max\_balance\_seconds | integer | NULL = no cap |
| rollover\_max\_seconds | integer | NULL = all, 0 = none |
| allow\_negative | boolean |  |
| requires\_approval | boolean |  |
| exclude\_weekends / exclude\_holidays | boolean |  |
| external\_source | text | NULL or bamboohr (read-only in Worklume) |
| archived\_at | timestamptz |  |

Unique: (organization\_id, lower(name)) WHERE deleted\_at IS NULL.

### `time_off_policy_members`

**Tenant · Audit A · PK (organization\_id, policy\_id, member\_id)**

| Column | Type | Notes |
| --- | --- | --- |
| policy\_id | uuid | → time\_off\_policies |
| member\_id | uuid |  |
| joined\_on | date | proration start |
| left\_on | date |  |

### `time_off_ledger`

Every balance movement. **Tenant · append-only · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| policy\_id | uuid | → time\_off\_policies |
| member\_id | uuid |  |
| kind | text | CHECK in (accrual, usage, adjustment, rollover, expiry, reversal, import) |
| amount\_seconds | integer | signed |
| effective\_on | date |  |
| request\_id | uuid | → time\_off\_requests, NULL |
| idempotency\_key | text | e.g. `accrual:{policy}:{member}:{period}` |
| note | text |  |
| created\_by\_member\_id | uuid | NULL = system |

Unique: (organization\_id, idempotency\_key). Indexes: (organization\_id, member\_id, policy\_id, effective\_on).

### `time_off_balances`

Cached sum of the ledger per member and policy. **Tenant · Audit N (rebuildable) · PK (organization\_id, policy\_id, member\_id)**

| Column | Type | Notes |
| --- | --- | --- |
| policy\_id | uuid |  |
| member\_id | uuid |  |
| balance\_seconds | integer |  |
| pending\_seconds | integer | requested, not yet approved |
| as\_of | timestamptz | last ledger row included |

### `time_off_requests`

**Tenant · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| member\_id | uuid |  |
| policy\_id | uuid | → time\_off\_policies |
| starts\_on / ends\_on | date |  |
| day\_parts | jsonb | per date: full, half, or seconds |
| total\_seconds | integer |  |
| reason | text |  |
| status | text | CHECK in (pending, approved, denied, cancelled) |
| reviewed\_by\_member\_id | uuid |  |
| reviewed\_at | timestamptz |  |
| review\_note | text |  |
| external\_ref | text | BambooHR request ID |

Indexes: (organization\_id, member\_id, starts\_on); (organization\_id, status).

### `holiday_calendars`

Preset calendars (global, `organization_id` NULL) and org-defined ones. **Tenant-or-global · Soft delete · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| organization\_id | uuid | NULL for presets |
| name | text | e.g. "India national holidays" |
| country\_code | char(2) |  |

### `holidays`

**Tenant-or-global · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| holiday\_calendar\_id | uuid | FK → holiday\_calendars |
| organization\_id | uuid | NULL for presets |
| date | date |  |
| name | text |  |
| paid\_seconds | integer | e.g. 8 h |
| recurring\_yearly | boolean |  |

Unique: (holiday\_calendar\_id, date).

### `holiday_assignments`

Who a calendar applies to. **Tenant · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| holiday\_calendar\_id | uuid | FK → holiday\_calendars |
| scope\_type | text | organization, member, country, employment\_type |
| scope\_value | text | member ID, country code or type |

Unique: (organization\_id, holiday\_calendar\_id, scope\_type, scope\_value).

### `overtime_policies`

**Tenant · Soft delete · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| name | text |  |
| weekly\_threshold\_seconds | integer | NULL = none |
| daily\_threshold\_seconds | integer | NULL = none |
| multiplier | numeric(4,2) | e.g. 1.50 |
| double\_time\_threshold\_seconds | integer | NULL |
| double\_time\_multiplier | numeric(4,2) | e.g. 2.00 |
| effective\_from | date | starts next work week |
| status | text | scheduled, active, archived |

### `overtime_policy_members`

**Tenant · Audit A · PK (organization\_id, policy\_id, scope\_type, scope\_id)**

| Column | Type | Notes |
| --- | --- | --- |
| policy\_id | uuid | → overtime\_policies |
| scope\_type | text | member or team |
| scope\_id | uuid |  |

### `overtime_calculations`

Weekly overtime result per member, recomputed when time changes and frozen when the timesheet is approved. **Tenant · Audit N**

| Column | Type | Notes |
| --- | --- | --- |
| member\_id | uuid |  |
| week\_start | date |  |
| policy\_id | uuid |  |
| regular\_seconds | integer |  |
| overtime\_seconds | integer |  |
| double\_time\_seconds | integer |  |
| frozen\_at | timestamptz |  |

Unique: (organization\_id, member\_id, week\_start).

## 9. Expenses, payroll, invoicing

### `expense_categories`

**Tenant · Soft delete · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| name | text |  |
| gl\_code | text | accounting export |
| requires\_receipt\_above\_minor | bigint | NULL = never |
| archived\_at | timestamptz |  |

Unique: (organization\_id, lower(name)) WHERE deleted\_at IS NULL.

### `expenses`

**Tenant · Soft delete · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| member\_id | uuid |  |
| project\_id | uuid | → projects |
| category\_id | uuid | → expense\_categories |
| spent\_on | date |  |
| amount\_minor | bigint | > 0 |
| currency | char(3) |  |
| description | text |  |
| billable | boolean |  |
| status | text | CHECK in (draft, submitted, approved, rejected, reimbursed, invoiced) |
| submitted\_at / reviewed\_at | timestamptz |  |
| reviewed\_by\_member\_id | uuid |  |
| review\_note | text |  |
| payment\_id | uuid | → payments, when reimbursed |
| invoice\_line\_id | uuid | → invoice\_lines, when billed |

Indexes: (organization\_id, member\_id, spent\_on); (organization\_id, status); (organization\_id, project\_id).

### `expense_receipts`

**Tenant · Soft delete · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| expense\_id | uuid | → expenses ON DELETE CASCADE |
| storage\_key | text | `orgs/{org}/receipts/…` |
| mime\_type | text | image/jpeg, image/png, application/pdf |
| bytes | integer | ≤ 10 MB |
| sha256 | bytea |  |
| scan\_status | text | pending, clean, infected |

### `payout_accounts`

Where a member is paid (provider account reference, never bank details). **Tenant · Soft delete · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| member\_id | uuid |  |
| provider | text | wise, paypal, payoneer, deel, gusto, remote, manual |
| external\_ref\_encrypted | bytea | provider recipient ID or email |
| currency | char(3) |  |
| status | text | pending, verified, failed |
| is\_default | boolean |  |

Unique: (organization\_id, member\_id) WHERE is\_default AND deleted\_at IS NULL.

### `payroll_runs`

One run per pay period (automatic or manual). **Tenant · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| period\_start / period\_end | date |  |
| pay\_schedule | text | weekly, biweekly, semimonthly, monthly |
| mode | text | automatic, manual |
| status | text | CHECK in (draft, pending\_approval, approved, processing, completed, partially\_failed, cancelled) |
| approval\_window\_ends\_at | timestamptz | automatic runs wait this long |
| approved\_by\_member\_id | uuid |  |
| totals | jsonb | gross by currency |

Unique: (organization\_id, period\_start, pay\_schedule) WHERE status <> 'cancelled'.

### `payments`

One payment to one member (payroll, bonus or one-off). **Tenant · Audit A · immutable once sent**

| Column | Type | Notes |
| --- | --- | --- |
| payroll\_run\_id | uuid | → payroll\_runs, NULL for one-off |
| member\_id | uuid |  |
| payout\_account\_id | uuid | → payout\_accounts |
| kind | text | payroll, bonus, reimbursement, adjustment |
| gross\_minor | bigint |  |
| currency | char(3) |  |
| status | text | CHECK in (draft, queued, sent, paid, failed, marked\_paid, voided) |
| provider | text |  |
| provider\_ref | text |  |
| idempotency\_key | text | sent to the provider |
| paid\_at | timestamptz |  |
| failure\_reason | text |  |

Unique: (organization\_id, idempotency\_key); (organization\_id, provider, provider\_ref). Indexes: (organization\_id, member\_id, created\_at).

### `payment_line_items`

How a payment was calculated. **Tenant · append-only · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| payment\_id | uuid | → payments ON DELETE CASCADE |
| kind | text | regular\_hours, overtime, double\_time, pto, holiday, fixed, bonus, expense, adjustment |
| quantity\_seconds | integer | NULL for amounts |
| rate\_minor | bigint |  |
| multiplier | numeric(4,2) |  |
| amount\_minor | bigint |  |
| source\_type / source\_id | text / uuid | timesheet, expense, time\_off\_request |

### `payslips`

**Tenant · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| payment\_id | uuid | → payments |
| storage\_key | text | PDF |
| generated\_at | timestamptz |  |

Unique: (organization\_id, payment\_id).

### `invoices`

Client invoices (org bills a client) and team invoices (contractor bills the org). **Tenant · Soft delete (drafts only) · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| kind | text | CHECK in (client, team) |
| client\_id | uuid | → clients, for client invoices |
| member\_id | uuid | → organization\_members, for team invoices |
| number | text | from invoice\_sequences |
| status | text | CHECK in (draft, sent, viewed, partially\_paid, paid, void) |
| issue\_on / due\_on | date |  |
| currency | char(3) |  |
| subtotal\_minor / tax\_minor / total\_minor / paid\_minor | bigint |  |
| tax\_rate | numeric(6,3) |  |
| notes | text |  |
| recurring\_schedule\_id | uuid | → recurring\_invoice\_schedules |
| pdf\_key | text |  |
| payment\_link\_url | text | e.g. PayPal |
| sent\_at / paid\_at | timestamptz |  |

Unique: (organization\_id, kind, number). Indexes: (organization\_id, client\_id, issue\_on); (organization\_id, status, due\_on).

### `invoice_lines`

**Tenant · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| invoice\_id | uuid | → invoices ON DELETE CASCADE |
| kind | text | time, expense, fixed, other |
| description | text |  |
| project\_id | uuid |  |
| member\_id | uuid |  |
| quantity | numeric(12,4) | hours or units |
| unit\_price\_minor | bigint |  |
| amount\_minor | bigint |  |
| period\_start / period\_end | date | time lines |

Time included on a sent invoice is locked (`time_entries.lock_reason = invoice`).

### `invoice_payments`

**Tenant · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| invoice\_id | uuid | → invoices |
| amount\_minor | bigint |  |
| paid\_on | date |  |
| method | text |  |
| reference | text |  |

### `recurring_invoice_schedules`

**Tenant · Soft delete · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| client\_id | uuid | → clients |
| cadence | text | weekly, biweekly, monthly |
| next\_run\_on | date |  |
| template | jsonb | projects, members, include expenses |
| auto\_send | boolean |  |
| paused\_at | timestamptz |  |

### `invoice_sequences`

Gap-free invoice numbering per org and kind. **Tenant · PK (organization\_id, kind)**

| Column | Type | Notes |
| --- | --- | --- |
| kind | text | client, team |
| prefix | text | e.g. INV- |
| next\_number | bigint | incremented with `SELECT … FOR UPDATE` |

## 10. Billing (Worklume subscriptions)

The payment provider (Stripe or Paddle, ADR-007) is the system of record for charges; these tables mirror it and hold entitlements.

### `plans`

**Global · PK code (text) · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| code | text | PK: free, starter, grow, team, enterprise (names to be set) |
| name | text |  |
| tier\_rank | smallint | upgrade/downgrade direction |
| min\_seats | smallint |  |
| is\_public | boolean |  |
| retired\_at | timestamptz | grandfathered plans stay valid |

### `addons`

**Global · PK code (text)**

| Column | Type | Notes |
| --- | --- | --- |
| code | text | insights, locations, silent\_agent, extra\_screenshots, extended\_retention, ai |
| name | text |  |
| per\_seat | boolean |  |
| available\_on\_plans | text\[\] |  |

### `prices`

**Global · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| product\_type | text | plan or addon |
| product\_code | text |  |
| interval | text | month, quarter, year |
| currency | char(3) |  |
| unit\_amount\_minor | bigint | per seat |
| provider | text | stripe, paddle |
| provider\_price\_id | text |  |
| active | boolean |  |

Unique: (product\_type, product\_code, interval, currency, provider) WHERE active.

### `plan_features`

The entitlement catalog. **Global · PK (plan\_code, feature\_key)**

| Column | Type | Notes |
| --- | --- | --- |
| plan\_code | text | FK → plans |
| feature\_key | text | e.g. `screenshots.monthly_limit`, `idle.enabled`, `integrations.max`, `retention.screenshots_days` |
| value | jsonb | boolean, number or `unlimited` |

### `billing_customers`

**Tenant · Audit A · PK (organization\_id)**

| Column | Type | Notes |
| --- | --- | --- |
| provider | text |  |
| provider\_customer\_id | text |  |
| billing\_email | citext |  |
| company\_name | text |  |
| address | jsonb |  |
| tax\_id | text | GSTIN, VAT ID |
| tax\_exempt | boolean |  |

Unique: (provider, provider\_customer\_id).

### `subscriptions`

**Tenant · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| provider | text |  |
| provider\_subscription\_id | text |  |
| plan\_code | text | FK → plans |
| interval | text |  |
| status | text | CHECK in (trialing, active, past\_due, suspended, cancelled) |
| seats\_purchased | integer |  |
| trial\_ends\_at | timestamptz |  |
| current\_period\_start / current\_period\_end | timestamptz |  |
| cancel\_at | timestamptz |  |
| cancelled\_at | timestamptz |  |
| pending\_change | jsonb | downgrade scheduled for period end |

Unique: (organization\_id) WHERE status <> 'cancelled'; (provider, provider\_subscription\_id).

### `subscription_items`

**Tenant · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| subscription\_id | uuid | → subscriptions |
| item\_type | text | plan, addon |
| code | text |  |
| quantity | integer | seats |
| provider\_item\_id | text |  |

Unique: (organization\_id, subscription\_id, item\_type, code).

### `seat_events`

Every change in billable seats; drives proration and seat sync to the provider. **Tenant · append-only · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| member\_id | uuid |  |
| event | text | occupied, released |
| reason | text | joined, role\_changed, removed, reactivated |
| seats\_after | integer |  |
| synced\_to\_provider\_at | timestamptz |  |

Indexes: (organization\_id, created\_at).

### `entitlements`

Resolved features for the org (plan + add-ons + overrides). **Tenant · Audit A · PK (organization\_id, feature\_key)**

| Column | Type | Notes |
| --- | --- | --- |
| feature\_key | text |  |
| value | jsonb |  |
| source | text | plan, addon, override, trial |
| expires\_at | timestamptz | overrides and trials |

### `usage_counters`

Metered limits, e.g. screenshots per seat per month on lower plans. **Tenant · Audit N**

| Column | Type | Notes |
| --- | --- | --- |
| metric | text | screenshots, app\_url\_records, ai\_tokens, integrations |
| member\_id | uuid | NULL for org-level metrics |
| period\_start | date |  |
| count | bigint | incremented in Redis, flushed every minute |

Unique: (organization\_id, metric, coalesce(member\_id, zero-uuid), period\_start).

### `billing_invoices`

Mirror of provider invoices for the in-app billing history. **Tenant · Audit N**

| Column | Type | Notes |
| --- | --- | --- |
| provider\_invoice\_id | text |  |
| number | text |  |
| status | text | draft, open, paid, void, uncollectible |
| amount\_due\_minor / amount\_paid\_minor / tax\_minor | bigint |  |
| currency | char(3) |  |
| period\_start / period\_end | timestamptz |  |
| hosted\_url / pdf\_url | text |  |
| issued\_at | timestamptz |  |

Unique: (provider\_invoice\_id).

### `billing_webhook_events`

Inbound provider events, processed once. **Global · Retained 1 year**

| Column | Type | Notes |
| --- | --- | --- |
| provider | text |  |
| provider\_event\_id | text |  |
| type | text |  |
| organization\_id | uuid | resolved from customer, NULL until resolved |
| payload | jsonb |  |
| received\_at / processed\_at | timestamptz |  |
| error | text |  |

Unique: (provider, provider\_event\_id).

## 11. Reports and rollups

### `saved_reports`

**Tenant · Soft delete · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| owner\_member\_id | uuid |  |
| report\_type | text | time\_activity, daily\_totals, work\_sessions, apps\_urls, manual\_edits, timesheet\_approvals, attendance, shift\_attendance, job\_sites, breaks, time\_off, limits, budgets, amounts\_owed, payments, invoices, invoice\_aging, expenses, audit, productivity |
| name | text |  |
| filters | jsonb | members, teams, projects, clients, date range rule |
| columns | text\[\] |  |
| group\_by | text\[\] |  |
| visibility | text | private, organization |
| favorite | boolean |  |

### `report_schedules`

**Tenant · Soft delete · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| saved\_report\_id | uuid | → saved\_reports |
| cadence | text | daily, weekly, monthly |
| day\_of\_week / day\_of\_month | smallint |  |
| local\_time | time |  |
| timezone | text |  |
| format | text | csv, pdf, xlsx |
| recipients | jsonb | member IDs and emails (emails limited to org domains unless owner allows) |
| paused\_at | timestamptz |  |
| next\_run\_at / last\_run\_at | timestamptz |  |

Indexes: (next\_run\_at) WHERE paused\_at IS NULL.

### `report_exports`

Asynchronous export jobs and their files. **Tenant · Retained 7 days · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| requested\_by\_member\_id | uuid | NULL for schedules |
| report\_schedule\_id | uuid |  |
| report\_type | text |  |
| params | jsonb | frozen filters and timezone |
| format | text |  |
| status | text | queued, running, ready, failed, expired |
| storage\_key | text |  |
| row\_count | integer |  |
| started\_at / finished\_at / expires\_at | timestamptz |  |
| error | text |  |

Indexes: (organization\_id, requested\_by\_member\_id, created\_at).

### `rollup_member_daily`

The main reporting fact: one row per member, org-local day, project and task. **Tenant · Audit N (rebuildable) · PK (organization\_id, member\_id, work\_date, project\_id, task\_key)**

| Column | Type | Notes |
| --- | --- | --- |
| member\_id | uuid |  |
| work\_date | date | org timezone |
| project\_id | uuid |  |
| task\_key | uuid | task ID or zero-uuid |
| tracked\_seconds | integer |  |
| active\_seconds | integer |  |
| keyboard\_seconds / mouse\_seconds | integer |  |
| manual\_seconds | integer |  |
| idle\_seconds | integer |  |
| billable\_seconds | integer |  |
| screenshot\_count | integer |  |
| core\_seconds / non\_core\_seconds / unproductive\_seconds | integer | from classifications |
| focus\_seconds | integer | 30-min uninterrupted blocks |
| cost\_minor / bill\_minor | bigint | at effective rates |
| updated\_at | timestamptz |  |

Indexes: (organization\_id, work\_date); (organization\_id, project\_id, work\_date).

### `rollup_usage_daily`

App and domain time per member per day. **Tenant · Audit N · retained as app/URL retention allows**

| Column | Type | Notes |
| --- | --- | --- |
| member\_id | uuid |  |
| work\_date | date |  |
| subject\_type | text | application, web\_domain |
| subject\_id | uuid |  |
| seconds | integer |  |
| rating | text | resolved classification |

PK (organization\_id, member\_id, work\_date, subject\_type, subject\_id).

### `rollup_invalidations`

Work queue of (member, day) keys to recompute after late or edited data. **Tenant · Audit N · PK (organization\_id, member\_id, work\_date)**

| Column | Type | Notes |
| --- | --- | --- |
| member\_id | uuid |  |
| work\_date | date |  |
| reason | text | ingest, edit, rate\_change, classification\_change |
| enqueued\_at | timestamptz |  |

## 12. Integrations, webhooks, API platform, outbox

### `integration_providers`

**Global · PK key (text)**

| Column | Type | Notes |
| --- | --- | --- |
| key | text | jira, asana, clickup, github, gitlab, trello, slack, msteams, quickbooks, xero, bamboohr, google\_calendar, microsoft\_calendar, zapier, wise, paypal, deel, gusto |
| name | text |  |
| category | text | project\_management, accounting, payroll, hr, communication, calendar, automation |
| auth\_type | text | oauth2, oauth1, api\_key |
| capabilities | jsonb | import\_tasks, write\_time, import\_time\_off, send\_messages… |
| min\_plan | text |  |
| enabled | boolean | kill switch |

### `integration_connections`

One authorised account of a provider for an org. **Tenant · Soft delete · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| provider\_key | text | FK → integration\_providers |
| connected\_by\_member\_id | uuid |  |
| external\_account\_id | text | e.g. Jira cloud ID |
| credentials\_encrypted | bytea | access/refresh tokens, KMS envelope |
| token\_expires\_at | timestamptz |  |
| scopes | text\[\] |  |
| settings | jsonb | write-back mode (hourly, daily, delayed, on\_complete, off), mappings |
| status | text | active, needs\_reauth, error, disconnected |
| last\_sync\_at | timestamptz |  |
| last\_error | text |  |

Unique: (organization\_id, provider\_key, external\_account\_id) WHERE deleted\_at IS NULL.

### `integration_project_links`

**Tenant · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| connection\_id | uuid | → integration\_connections |
| project\_id | uuid | → projects |
| external\_project\_id | text |  |
| sync\_cursor | text | provider pagination/updated-since cursor |
| options | jsonb | include epics, subtasks |

Unique: (organization\_id, connection\_id, external\_project\_id); (organization\_id, project\_id).

### `integration_user_links`

**Tenant · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| connection\_id | uuid |  |
| member\_id | uuid |  |
| external\_user\_id | text |  |
| per\_user\_credentials\_encrypted | bytea | providers that need per-user OAuth (Trello, GitHub) |

Unique: (organization\_id, connection\_id, external\_user\_id).

### `integration_entity_maps`

Maps every synced entity to its external twin; the basis for two-way sync and loop prevention. **Tenant · Audit N**

| Column | Type | Notes |
| --- | --- | --- |
| connection\_id | uuid |  |
| entity\_type | text | task, time\_entry, client, time\_off\_request, invoice |
| local\_id | uuid |  |
| external\_id | text |  |
| external\_version | text | etag / updated\_at |
| content\_hash | bytea | last synced state; skip echo updates |
| synced\_at | timestamptz |  |

Unique: (organization\_id, connection\_id, entity\_type, external\_id); (organization\_id, connection\_id, entity\_type, local\_id).

### `integration_sync_runs`

**Tenant · Retained 90 days · Audit N**

| Column | Type | Notes |
| --- | --- | --- |
| connection\_id | uuid |  |
| direction | text | inbound, outbound |
| trigger | text | schedule, webhook, manual |
| status | text | running, succeeded, partial, failed |
| stats | jsonb | created, updated, skipped, failed |
| started\_at / finished\_at | timestamptz |  |
| error | text |  |

### `integration_inbound_events`

Webhooks received from providers. **Global (org resolved on processing) · Retained 30 days**

| Column | Type | Notes |
| --- | --- | --- |
| provider\_key | text |  |
| external\_event\_id | text |  |
| organization\_id | uuid | NULL until matched |
| connection\_id | uuid |  |
| signature\_valid | boolean |  |
| payload | jsonb |  |
| received\_at / processed\_at | timestamptz |  |

Unique: (provider\_key, external\_event\_id).

### `webhook_endpoints`

Customer-registered outbound webhooks. **Tenant · Soft delete · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| url | text | HTTPS only; private IP ranges blocked (SSRF) |
| description | text |  |
| events | text\[\] | subscribed event types or `*` |
| secret\_encrypted | bytea | HMAC-SHA256 signing secret |
| previous\_secret\_encrypted | bytea | valid 24 h after rotation |
| status | text | pending\_verification, active, disabled |
| failure\_streak | integer | disabled after 50 consecutive failures |
| created\_by\_member\_id | uuid |  |

Indexes: (organization\_id, status).

### `webhook_deliveries`

**Tenant · Partitioned by created\_at · Retained 30 days · Audit N**

| Column | Type | Notes |
| --- | --- | --- |
| endpoint\_id | uuid |  |
| event\_id | uuid | outbox event |
| event\_type | text |  |
| attempt | smallint |  |
| status | text | pending, succeeded, failed, abandoned |
| response\_status | smallint |  |
| response\_ms | integer |  |
| next\_attempt\_at | timestamptz | backoff up to 24 h |
| delivered\_at | timestamptz |  |

Indexes: (organization\_id, endpoint\_id, created\_at); (next\_attempt\_at) WHERE status = 'pending'.

### `outbox_events`

Transactional outbox: written in the same transaction as the change; relayed to webhooks, WebSockets, integrations, notifications and the AI plane. **Tenant · Partitioned by occurred\_at (daily) · Retained 7 days after publish**

| Column | Type | Notes |
| --- | --- | --- |
| id | uuid | event ID (UUIDv7) |
| aggregate\_type | text |  |
| aggregate\_id | uuid |  |
| event\_type | text | e.g. `time_entry.updated` |
| event\_version | smallint | schema version |
| payload | jsonb |  |
| actor\_member\_id | uuid |  |
| occurred\_at | timestamptz |  |
| published\_at | timestamptz | NULL until relayed |

Indexes: (occurred\_at) WHERE published\_at IS NULL.

### `idempotency_keys`

Stored responses for `Idempotency-Key` requests. **Tenant · Retained 24 h · PK (organization\_id, principal\_id, key)**

| Column | Type | Notes |
| --- | --- | --- |
| principal\_id | uuid | member, token or device |
| key | text | client-supplied, ≤ 255 chars |
| method / path | text |  |
| request\_hash | bytea | a different body with the same key → 422 |
| status | text | in\_progress, completed |
| response\_status | smallint |  |
| response\_body | jsonb |  |
| expires\_at | timestamptz |  |

## 13. Notifications

### `notification_types`

**Global · PK key (text)**

| Column | Type | Notes |
| --- | --- | --- |
| key | text | e.g. `shift.late`, `timesheet.submitted`, `budget.threshold`, `idle.long`, `activity.low`, `insight.unusual` |
| audience | text | self, managers, admins |
| default\_channels | text\[\] | in\_app, email, push, slack, msteams |
| mandatory | boolean | security notices cannot be turned off |

### `notification_preferences`

**Tenant · Audit N**

| Column | Type | Notes |
| --- | --- | --- |
| member\_id | uuid |  |
| type\_key | text | FK → notification\_types |
| channel | text |  |
| enabled | boolean |  |
| digest | text | immediate, daily, weekly |

Unique: (organization\_id, member\_id, type\_key, channel).

### `alert_rules`

Org-configured alerts (low activity below X%, overtime, budget, missed shift, long idle). **Tenant · Soft delete · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| rule\_type | text | low\_activity, high\_idle, overtime, budget, shift, unusual\_activity, device\_offline |
| params | jsonb | thresholds, windows |
| scope\_type / scope\_id | text / uuid | organization, team, member |
| recipients | jsonb | roles or members |
| enabled | boolean |  |

### `notifications`

In-app inbox. **Tenant · Partitioned by created\_at · Retained 90 days · Audit N**

| Column | Type | Notes |
| --- | --- | --- |
| member\_id | uuid | recipient |
| type\_key | text |  |
| title | text |  |
| body | text |  |
| data | jsonb | deep-link target |
| dedupe\_key | text | one notification per event per member |
| read\_at | timestamptz |  |

Indexes: (organization\_id, member\_id, created\_at) WHERE read\_at IS NULL. Unique: (organization\_id, dedupe\_key, created\_at).

### `notification_deliveries`

**Tenant · Retained 30 days · Audit N**

| Column | Type | Notes |
| --- | --- | --- |
| notification\_id | uuid | soft reference |
| channel | text |  |
| status | text | queued, sent, delivered, bounced, failed |
| provider\_message\_id | text |  |
| attempted\_at | timestamptz |  |
| error | text |  |

### `push_tokens`

**Tenant · Audit N**

| Column | Type | Notes |
| --- | --- | --- |
| device\_id | uuid | → devices |
| member\_id | uuid |  |
| platform | text | apns, fcm |
| token | text |  |
| last\_seen\_at | timestamptz |  |
| revoked\_at | timestamptz | invalid token feedback |

Unique: (platform, token).

## 14. AI

These tables hold AI outputs only. AI inputs (features, aggregates) live in the analytics store (Architecture §18).

### `ai_model_versions`

Registry of model and prompt versions, so every output is traceable. **Global · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| capability | text | summary, nl\_query, anomaly, classification, workload |
| provider | text | bedrock, internal |
| model\_id | text |  |
| prompt\_version | text |  |
| rules\_version | text | for deterministic detectors |
| status | text | shadow, active, retired |

### `ai_insights`

Generated summaries, workload findings and recommendations. **Tenant · Audit A · Retained 13 months**

| Column | Type | Notes |
| --- | --- | --- |
| insight\_type | text | daily\_summary, weekly\_summary, workload, recommendation, trend |
| subject\_type | text | member, team, project, organization |
| subject\_id | uuid |  |
| audience | text | self, managers, both |
| period\_start / period\_end | date |  |
| severity | text | info, notice, warning |
| title | text |  |
| body | text |  |
| evidence | jsonb | metric references (rollup keys, values); never raw screenshots or URLs |
| model\_version\_id | uuid | FK → ai\_model\_versions |
| status | text | new, seen, acknowledged, dismissed, actioned |
| feedback | smallint | -1, 0, 1 |
| expires\_at | timestamptz |  |

Indexes: (organization\_id, subject\_type, subject\_id, period\_start); (organization\_id, audience, status).

### `ai_anomalies`

Unusual-activity detections needing human review. **Tenant · Audit A · Retained 13 months**

| Column | Type | Notes |
| --- | --- | --- |
| member\_id | uuid |  |
| detected\_for\_date | date |  |
| rule\_key | text | e.g. constant\_activity, keyboard\_without\_mouse, suspicious\_app, automated\_input |
| confidence | text | slight, moderate, high |
| score | real |  |
| evidence | jsonb | slot IDs, metrics |
| explanation | text | plain-language reason |
| model\_version\_id | uuid |  |
| status | text | open, confirmed, dismissed |
| reviewed\_by\_member\_id | uuid |  |
| reviewed\_at | timestamptz |  |
| review\_note | text |  |

Unique: (organization\_id, member\_id, detected\_for\_date, rule\_key). Indexes: (organization\_id, status, detected\_for\_date).

### `ai_queries`

Natural-language questions and answers. **Tenant · Audit A · Retained 90 days**

| Column | Type | Notes |
| --- | --- | --- |
| member\_id | uuid | who asked |
| question | text |  |
| resolved\_plan | jsonb | the semantic query(ies) executed: metric, filters, grouping |
| answer | text |  |
| citations | jsonb | report links and figures used |
| model\_version\_id | uuid |  |
| tokens\_in / tokens\_out | integer |  |
| cost\_micros | bigint |  |
| latency\_ms | integer |  |
| status | text | answered, refused, failed |
| feedback | smallint |  |

Indexes: (organization\_id, member\_id, created\_at).

### `ai_usage_ledger`

AI consumption for quotas and cost control. **Tenant · append-only**

| Column | Type | Notes |
| --- | --- | --- |
| capability | text |  |
| tokens\_in / tokens\_out | integer |  |
| cost\_micros | bigint |  |
| source\_type / source\_id | text / uuid | insight, query, anomaly batch |

Indexes: (organization\_id, created\_at).

## 15. Audit, privacy, SSO/SCIM, platform

### `audit_logs`

Tamper-evident log of every significant action. **Tenant (NULL for platform events) · Partitioned by occurred\_at · append-only · Retained 2 years (Enterprise configurable up to 7)**

| Column | Type | Notes |
| --- | --- | --- |
| organization\_id | uuid | NULL for platform-level events |
| occurred\_at | timestamptz |  |
| actor\_type | text | member, api\_token, oauth\_app, device, system, platform\_admin, scim |
| actor\_id | uuid |  |
| actor\_member\_id | uuid |  |
| on\_behalf\_of | uuid | platform admin under break-glass |
| action | text | `domain.entity.verb`, e.g. `timesheet.approved`, `screenshot.deleted`, `policy.changed` |
| target\_type / target\_id | text / uuid |  |
| changes | jsonb | before/after, sensitive fields redacted |
| ip | inet |  |
| user\_agent | text |  |
| request\_id | uuid |  |
| prev\_hash / hash | bytea | per-org hash chain: hash = SHA-256(prev\_hash ‖ row) |

The app role has INSERT and SELECT only (no UPDATE or DELETE). Indexes: (organization\_id, occurred\_at); (organization\_id, target\_type, target\_id); (organization\_id, actor\_member\_id, occurred\_at). A nightly job anchors each org's latest hash to an S3 Object Lock bucket.

### `sso_connections`

**Tenant · Soft delete · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| protocol | text | saml, oidc |
| idp\_entity\_id | text |  |
| sso\_url | text |  |
| certificates | text\[\] | X.509, rotation supported |
| oidc\_issuer / oidc\_client\_id | text |  |
| oidc\_client\_secret\_encrypted | bytea |  |
| attribute\_map | jsonb | email, name, groups |
| enforced | boolean | password login disabled for org members |
| jit\_provisioning | boolean |  |
| default\_role\_id | uuid | → roles |
| status | text | draft, testing, active |

### `organization_domains`

Verified email domains for SSO discovery (not custom URLs). **Tenant · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| domain | text |  |
| verification\_token | text | DNS TXT |
| verified\_at | timestamptz |  |

Unique: domain WHERE verified\_at IS NOT NULL (one org per verified domain).

### `scim_tokens`

**Tenant · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| token\_hash | bytea |  |
| created\_by\_member\_id | uuid |  |
| last\_used\_at | timestamptz |  |
| revoked\_at | timestamptz |  |

Unique: token\_hash.

### `monitoring_notices`

Versioned monitoring disclosure employees must see (what is tracked, when, who can see it). **Tenant · append-only · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| version | integer |  |
| content | text | generated from current policies, editable |
| content\_hash | bytea |  |
| published\_by\_member\_id | uuid |  |
| published\_at | timestamptz |  |

Unique: (organization\_id, version).

### `consent_records`

Acknowledgement of each notice version by each member. **Tenant · append-only · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| member\_id | uuid |  |
| notice\_id | uuid | → monitoring\_notices |
| acknowledged\_at | timestamptz |  |
| device\_id | uuid | where acknowledged |
| ip | inet |  |
| withdrawn\_at | timestamptz | tracking stops for this member when withdrawn |

Unique: (organization\_id, member\_id, notice\_id).

### `data_subject_requests`

Access, export, correction and erasure requests. **Tenant · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| subject\_member\_id | uuid |  |
| request\_type | text | access, export, rectify, erase, restrict |
| status | text | received, verifying, in\_progress, completed, rejected |
| due\_at | timestamptz | legal deadline |
| handled\_by\_member\_id | uuid |  |
| export\_key | text | S3 key of the export bundle |
| completed\_at | timestamptz |  |
| notes | text |  |

### `retention_runs`

**Tenant · append-only · Retained 2 years**

| Column | Type | Notes |
| --- | --- | --- |
| data\_class | text | screenshots, app\_url, location, activity, time, audit, exports |
| cutoff | timestamptz |  |
| rows\_deleted | bigint |  |
| objects\_deleted | bigint | S3 objects |
| started\_at / finished\_at | timestamptz |  |
| status | text |  |

### `legal_holds`

Suspends retention deletes for a scope. **Tenant · Audit A**

| Column | Type | Notes |
| --- | --- | --- |
| scope\_type | text | organization, member |
| scope\_id | uuid |  |
| data\_classes | text\[\] |  |
| reason | text |  |
| created\_by\_member\_id | uuid |  |
| released\_at | timestamptz |  |

### `platform_access_grants`

Break-glass access for Worklume staff. **Global · Audit A (platform)**

| Column | Type | Notes |
| --- | --- | --- |
| organization\_id | uuid | FK → organizations |
| staff\_user\_id | uuid | internal staff directory ID |
| reason | text |  |
| ticket\_ref | text |  |
| approved\_by | uuid | second staff member |
| scope | text | read\_only, support\_write |
| starts\_at / expires\_at | timestamptz | max 4 h |
| revoked\_at | timestamptz |  |

### `feature_flags`

**Global**

| Column | Type | Notes |
| --- | --- | --- |
| key | text | PK |
| description | text |  |
| default\_enabled | boolean |  |
| rules | jsonb | percentage, org allowlist, plan |

**Framework tables** (Laravel, not tenant data): `migrations`, `failed_jobs`, `job_batches`, `cache_locks` (Redis is the primary cache and queue store).

**Count:** 142 designed tables plus 4 framework tables. 110 are tenant-owned, 27 are global, 4 are shared catalogs (system rows global, custom rows tenant: `roles`, `role_permissions`, `holiday_calendars`, `holidays`), and 1 is the `organizations` root.
