# Worklume — Phase 2 Architecture

Sep 28, 2026 · @Suni Kumar

Worklume is one multi-tenant SaaS: a Laravel modular monolith on PostgreSQL, a Tauri/Rust desktop agent, a React Native app and a React web app, on AWS. Tenant isolation is enforced in four independent layers. This tab holds the architecture; the Database and API tabs hold the full table and endpoint catalogs. Built on the Phase 1 research (Hubstaff capability analysis).

## 1. Architecture overview

Worklume is a modular monolith with two satellite services. The core is one Laravel application split into strictly bounded domain modules. The satellites are a high-volume ingestion path for agent telemetry and a separate AI plane. Phase 1 showed that everything rolls up from the 10-minute activity slot, so the design treats slot ingestion as the hot path and everything else as ordinary CRUD.

**Three runtime shapes**

1. **Web → API → domain layer → database.** The React web app calls the REST API. Controllers are thin. Each call goes to one domain module's application service, which enforces policy and writes through repositories to PostgreSQL. Side effects leave via a transactional outbox.
2. **Desktop → Rust core → OS native APIs → sync engine → API.** The Tauri agent's Rust core samples input, captures screenshots and reads the active app through per-OS adapters. It writes everything to a local encrypted SQLite queue first. A sync engine drains that queue to the API in idempotent batches.
3. **Mobile → API → backend.** The React Native app uses the same REST API, with a local queue for offline time and location.

**The ten decisions that shape the system**

| # | Decision | Why |
| --- | --- | --- |
| 1 | Modular monolith in Laravel (PHP 8.4), not microservices | One team, fast delivery, matches your Laravel background; module boundaries are enforced so services can be split out later |
| 2 | Shared database, shared schema, `organization_id` everywhere | Cheapest to run at thousands of small tenants; isolation enforced in 4 layers (§4) |
| 3 | PostgreSQL row-level security as a backstop to app scopes | A missed `where` clause cannot leak data |
| 4 | Composite foreign keys `(organization_id, id)` | The database itself refuses a row that points at another tenant's row |
| 5 | Tokens are bound to one organization | The client never sends an org ID for tenant data; switching org mints a new token |
| 6 | Client-generated UUIDv7 IDs for agent data | Offline events carry their final ID, so retries are naturally idempotent |
| 7 | Monthly-partitioned time-series tables | Activity, app/URL, screenshot and location data grow fastest; old partitions drop cheaply at retention time |
| 8 | Reporting on pre-aggregated daily rollups, then a columnar store when needed | Reports stay fast without a warehouse on day one (§15) |
| 9 | Tauri 2 + Rust core for the agent, React Native for mobile | One codebase per surface; native code only where the OS demands it |
| 10 | AI runs in a separate plane that reads derived data only | Transactional data never flows directly into an LLM; outputs are stored as reviewable insights (§18) |

**Scope note.** The domain list you gave has 21 domains. Phase 1 also requires payroll, invoicing and privacy, so this design adds **Finance** (payroll, payouts, client and team invoices) and **Privacy** (consent, data requests, retention, legal holds) as domains 22 and 23.

## 2. System diagram

All traffic enters through one edge and reaches one Laravel codebase, deployed as five process types. The only bypass is screenshot bytes, which the agent uploads straight to S3 with a presigned URL.

&#91;embedded content: System diagram · clients, edge, 5 process types, data tier, AI plane\]

- **Ingest** is the same codebase as the API, deployed as its own service. It can then scale on agent traffic without touching the web API.
- **Screenshots** go agent → S3 directly. S3 emits an event to SQS, and a worker finishes processing (§14).
- **Third-party integrations** (Jira, QuickBooks and others) are called by workers through the Integrations domain; they are not drawn.

## 3. Domain architecture

23 domains, arranged in six layers. A domain may call the public service of a domain in a lower layer, and learns about higher layers only through events. Time Tracking is the centre of gravity: almost every higher domain reads from it.

&#91;embedded content: Domain layers · 23 domains in 6 layers\]

**Boundary rules**

1. Each domain is a Laravel module (`app/Domains/<Name>`) with its own models, services, policies, events and migrations. It owns its tables outright.
2. No domain writes another domain's tables. Reads across domains go through the owner's query service or a read model, never a raw join in controller code. Reporting is the one allowed exception: it reads rollup tables that other domains publish into.
3. Cross-domain side effects travel as domain events through the outbox (for example, `TimeEntryApproved` → Finance recalculates amounts owed).
4. Architecture tests (Pest + deptrac-style rules) fail the build when a module imports another module's internals.
5. Every domain except Identity and the global catalogs is tenant-scoped (§4).

| Domain | Owns | Publishes (examples) | May call |
| --- | --- | --- | --- |
| Identity | users, credentials, MFA, passkeys, sessions, tokens, SSO identities | UserRegistered, MfaEnabled | — |
| Organizations | organizations, members, invitations, policies, roles, job types | MemberJoined, MemberRemoved, PolicyChanged | Identity |
| Teams | teams, team members | TeamChanged | Organizations |
| Billing | subscriptions, seats, entitlements, usage counters, provider invoices | EntitlementsChanged, OrgSuspended | Organizations |
| Notifications | preferences, inbox, deliveries, push tokens, alert rules | — (consumes events) | Organizations |
| Audit | audit log | — (consumes events) | — |
| Privacy | consent, notices, data requests, retention runs, legal holds | RetentionRunCompleted | Organizations |
| Projects | clients, projects, project members, rates, budgets | ProjectArchived, BudgetThresholdReached | Organizations, Teams |
| Tasks | tasks, assignees | TaskCompleted, TaskAssigned | Projects |
| Time Tracking | timers, time entries, edits, idle periods, manual requests, timesheets | TimerStarted/Stopped, TimeEntryChanged, TimesheetApproved | Projects, Tasks, Devices |
| Devices | devices, heartbeats, releases, device commands | DeviceRevoked, CapabilitiesChanged | Organizations |
| Activity | activity slots | SlotsIngested | Time Tracking, Devices |
| Applications | app catalog, app usage, classifications | UsageIngested | Activity |
| Websites | domain catalog, URL usage | UsageIngested | Activity |
| Screenshots | screenshots, access log | ScreenshotReady, ScreenshotDeleted | Activity, Devices |
| Attendance | attendance records, breaks, job sites, visits, locations, office networks | ShiftLate, ShiftMissed, SiteEntered | Scheduling, Time Tracking |
| Scheduling | shift series, shifts | ShiftCreated | Organizations, Teams |
| PTO | policies, ledger, balances, requests, holidays, overtime policies | TimeOffApproved | Organizations |
| Expenses | categories, expenses, receipts | ExpenseApproved | Projects |
| Finance | payroll runs, payments, payslips, invoices, payout accounts | PaymentSent, InvoiceSent | Time Tracking, PTO, Expenses, Projects |
| Reports | saved reports, schedules, exports, rollups | ReportReady | Read models of all lower layers |
| AI | insights, anomalies, queries, AI usage | InsightCreated | Reports (read), AI plane |
| Integrations | connections, links, entity maps, sync runs, outbound webhooks | IntegrationFailed | Public services of Projects, Tasks, Time, PTO, Finance |

## 4. Multi-tenant architecture

Worklume uses a pooled model: one database, one schema, `organization_id` on every tenant row. Isolation is enforced by four independent layers, so a cross-tenant leak needs four separate failures, and an automated test suite checks every endpoint for it.

**Model choice**

| Model | Isolation | Cost at 10k small orgs | Verdict |
| --- | --- | --- | --- |
| Database per tenant | Strongest | Very high; migrations × 10k | Rejected for default; possible later for enterprise "silo" cells |
| Schema per tenant | Strong | High; catalog bloat, slow migrations | Rejected |
| Shared schema + `organization_id` + RLS | Strong with the layers below | Lowest | **Chosen** |

**The four enforcement layers**

| Layer | Mechanism | Fails closed because |
| --- | --- | --- |
| 1. Context | Every access token carries one `org_id` claim. `ResolveTenant` middleware loads the member's active membership and sets an immutable `TenantContext` for the request. | A token without an active membership gets 401; there is no org parameter to tamper with |
| 2. Authorization | Laravel Policies check RBAC permissions for the member (§12). Route-model binding resolves IDs through the tenant scope. | A foreign ID returns 404, never 403, so existence doesn't leak |
| 3. Query scope | `BelongsToOrganization` trait: a global scope adds `organization_id = ctx`, and a `creating` hook stamps it and rejects any other value. Raw queries are banned in domain code by a lint rule; `withoutGlobalScopes()` is allowed only in allowlisted platform code. | No context → the scope throws, not "no filter" |
| 4. Database | Row-level security on every tenant table: `USING (organization_id = current_setting('app.org_id', true)::uuid)` plus the same `WITH CHECK`. The app role `worklume_app` does not own tables and lacks `BYPASSRLS`. | Unset setting → NULL → zero rows |

**How each concern is handled**

- **`organization_id`.** `uuid NOT NULL` on every tenant table, second column after `id`. It is set by the server only, never read from request bodies.
- **Setting the RLS context.** Middleware opens a transaction per request and runs `SET LOCAL app.org_id = '<uuid>'`. `SET LOCAL` expires with the transaction, so it is safe with Octane's persistent connections and with PgBouncer in transaction mode. Queue jobs do the same inside their own transaction.
- **Policies.** Every controller action calls `authorize()`; a CI check fails any route without a policy. Policies receive the `TenantContext`, never a raw org ID.
- **Global scopes.** Applied to every tenant model through the base `TenantModel` class. An architecture test fails if a model in a tenant domain does not extend it.
- **Database constraints.** Every tenant table has `UNIQUE (organization_id, id)`. Child tables reference parents with a composite key, e.g. `FOREIGN KEY (organization_id, project_id) REFERENCES projects (organization_id, id)`. A time entry pointing at another tenant's project is rejected by PostgreSQL itself.
- **Unique indexes.** Always lead with `organization_id`, e.g. `UNIQUE (organization_id, lower(name)) WHERE deleted_at IS NULL`. Two tenants can both have a project called "Website".
- **Global tables.** Only identity and catalogs are global: `users`, `permissions`, `plans`, `applications`, `web_domains`, `integration_providers`, `agent_releases`, `notification_types`. `users` holds login identity only; everything a tenant knows about a person lives in `organization_members`, and all tenant data references `member_id`, not `user_id`.
- **Caching.** All tenant cache access goes through a `TenantCache` wrapper that prefixes keys `org:{orgId}:` and tags them by org. There is no un-prefixed cache API in domain code. Entitlements and resolved policies are cached per org and invalidated by events.
- **Queues.** Every job payload carries `organization_id`. A job middleware restores `TenantContext` and the RLS setting before `handle()` runs; a job without an org is rejected unless it is marked `PlatformJob`. Heavy queues are rate-limited per org, so one large tenant cannot starve others.
- **Storage.** S3 keys are computed by the server: `orgs/{orgId}/screenshots/{yyyy}/{mm}/{memberId}/{id}.webp`. The client never supplies a key. Objects are encrypted with KMS using encryption context `{org_id}`, so a decrypt with the wrong context fails. Presigned URLs are per object, GET or PUT only, 60–300 s, issued after a policy check. Buckets block all public access.
- **WebSockets.** Laravel Reverb private channels are named `private-org.{orgId}.…` (e.g. `private-org.{orgId}.member.{memberId}`). The channel-auth callback checks that the token's org equals the channel's org and that the member holds the needed permission. Broadcasts are emitted only from the outbox, which already carries the org.
- **Logs and traces.** Tagged with `org_id` and `request_id`; no screenshot contents or URLs in logs.
- **Platform staff access.** A separate admin app. Access to a tenant needs a time-limited break-glass grant with a reason and ticket. It is fully audited, and Enterprise customers can be notified.

**Proving it.** A cross-tenant test suite creates two organizations and replays every API endpoint with org A's token against org B's IDs; every call must return 404 or 422. A second suite runs raw SQL as `worklume_app` without `app.org_id` and expects zero rows from every tenant table. Both run in CI on every merge, and an external penetration test before launch focuses on tenant isolation.

## 5. Desktop architecture

One Tauri 2 application ships to macOS, Windows and Ubuntu. About 80% of the code — the tracking rules, storage and sync — is shared Rust. Each OS gets a thin adapter that implements the same traits, and the agent reports which capabilities actually work on the machine.

&#91;embedded content: Desktop agent · frontend, core, storage, sync, 4 OS adapters\]

```text
desktop/
├── frontend/          React + TypeScript UI rendered in the Tauri webview
├── core/              Rust crate: tracker, slots, screenshots, idle, policy, auth
├── sync/              Rust crate: outbox drain, upload, retry, token refresh
├── storage/           Rust crate: SQLCipher database, migrations, spool files
└── platform/          Rust crate: traits + per-OS implementations
    ├── macos/
    ├── windows/
    └── linux/
        ├── x11/
        └── wayland/
```

| Component | Responsibilities | Must never |
| --- | --- | --- |
| `frontend/` | Timer controls, project/to-do picker, idle-resolution dialog, break button, permission wizard, "my data" view (own screenshots, activity), settings. Talks to Rust only through typed Tauri commands and events. | Hold tracking state or call the API directly for tracking data |
| `core/` | Tracker state machine (stopped → running → idle-pending → paused). 10-min slot aggregation. Screenshot scheduling (random moment per slot, per-policy count). Idle detection and resolution. Policy engine (applies server config: frequency, blur, app/URL level, limits, auto-start). Capability detection. Tamper signals (clock jumps, suspicious input). | Call OS APIs directly (always through `platform/` traits) |
| `sync/` | Drains the outbox in ordered, idempotent batches. Presigned S3 uploads for screenshots. Exponential backoff with jitter. Access-token refresh; re-login prompt when the refresh token dies. Server clock-offset tracking. Applies server responses (accepted, duplicate, conflict). Pulls remote config and device commands. | Drop an event without a server acknowledgement |
| `storage/` | Encrypted SQLite (SQLCipher, WAL) holding the journal, outbox, slots, pending screenshots metadata, config cache. Screenshot spool directory with encrypted files. Key stored in the OS keychain (Keychain, DPAPI/Credential Manager, Secret Service). Schema migrations. Size caps with oldest-first eviction warnings. | Store tokens or keys in plain files |
| `platform/` (traits) | `ScreenCapture`, `InputActivity`, `IdleSource`, `ActiveWindow`, `BrowserUrl`, `Tray`, `Autostart`, `Notifier`, `SecretStore`, `PowerEvents` (sleep/wake), `NetworkIdentity` (public IP, router MAC hash). | Contain business rules |
| `platform/macos` | ScreenCaptureKit; `CGEventSourceSecondsSinceLastEventType` for idle; CGEvent tap for per-second activity (Input Monitoring); NSWorkspace for front app; AppleScript/Accessibility for URLs; TCC permission checks; SMAppService login item; Sparkle-style signed updates via the Tauri updater; notarized builds. | — |
| `platform/windows` | Windows.Graphics.Capture / Desktop Duplication; `GetLastInputInfo` + Raw Input; `GetForegroundWindow`; UI Automation for browser URLs; Run-key auto-start; the tracker runs in the user session; an optional Windows service handles updates and restarts the agent if killed. | Capture from a service (session 0 cannot see the desktop) |
| `platform/linux/x11` | XShm/XGetImage capture; XInput2 activity; EWMH `_NET_ACTIVE_WINDOW`; AT-SPI or the Worklume browser extension for URLs; StatusNotifierItem tray; XDG autostart. | — |
| `platform/linux/wayland` | Detected via `XDG_SESSION_TYPE`. xdg-desktop-portal Screenshot/ScreenCast (with consent) or compositor-specific paths; ext-idle-notify-v1 or Mutter IdleMonitor for idle and per-second activity (no keyboard/mouse split); GNOME Shell extension and KWin script for active window; browser extension for URLs. Unavailable capabilities are reported to the server and shown to admins. | Pretend to support a capability it cannot measure |

**Process model.** The agent is tray-resident with the main window hidden by default, single-instance, and starts at login. A watchdog restarts the tracker after a crash; the journal lets it resume the current slot. Signed builds only: Apple notarization, Windows Authenticode (EV certificate for SmartScreen), and a signed apt repository plus .deb/.rpm for Linux. Flatpak and Snap are avoided because their sandboxes block monitoring APIs.

**Capability reporting.** On start and every change, the agent sends a capability map (e.g. `screenshots: granted | denied | unsupported`). The server stores it on the device record, and the dashboard shows why a member has no screenshots instead of silently showing gaps.

## 6. Mobile architecture

One React Native app built with Expo (dev-client builds, not Expo Go) for iOS and Android. It uses the same REST API and the same offline outbox pattern as the desktop agent. Native code is limited to background location and geofencing.

| Concern | Design |
| --- | --- |
| Code sharing | A monorepo `packages/` holds the OpenAPI-generated API client, Zod schemas, domain types and formatting shared by web and mobile. UI components are not shared (different primitives). |
| Auth | OAuth 2.0 authorization code + PKCE in the system browser. Tokens are stored in Keychain / Android Keystore via `expo-secure-store`. Biometric unlock optional. |
| Offline | SQLite (`op-sqlite`) outbox with the same event envelope as desktop (§13): client UUIDv7, sequence number, device clock and monotonic time. Syncs on reconnect and in background fetch windows. |
| Timer | Start/stop, project/to-do, breaks, manual-time requests, timesheet view and submit, time-off requests. Mobile time carries no activity %, as in Hubstaff. |
| Background location | A native location module. Option A: Transistorsoft `react-native-background-geolocation` (commercial licence, handles OEM battery killers well). Option B: `expo-location` + `expo-task-manager` (free, weaker on some Android OEMs). Recommendation: prototype both, lean A. |
| iOS specifics | "When in use" then "Always" permission, precise location, significant-change monitoring when stationary, region monitoring for job sites (iOS limit: 20 regions per app, so the app registers the nearest 20 and refreshes as the user moves). |
| Android specifics | Foreground service of type `location` with a persistent notification while tracking; background-location declaration for Google Play review; up to 100 geofences per app. |
| Tracking modes | Off, timer-only, shifts-only, always — set by server policy; the app shows the current mode to the employee at all times. |
| Push | APNs and FCM via Expo Notifications; tokens stored per device (`push_tokens`). |
| Updates | Store releases for native changes; EAS Update for JavaScript-only fixes, pinned to the native runtime version. |
| Not on mobile | Screenshots, keyboard/mouse activity, app/URL tracking (matches Hubstaff and the OS rules). |

## 7. Database architecture

One PostgreSQL 17 database on Amazon RDS, with conventions that make tenant isolation, offline idempotency and retention cheap. The full catalog is in the Database tab.

**Engine and extensions.** RDS for PostgreSQL 17 (Multi-AZ) with one read replica for reports. Aurora PostgreSQL is the upgrade path when replica count or I/O grows. Extensions: `pgcrypto`, `citext`, `btree_gist` (no-overlap constraints), `pg_partman` (partitions), `postgis` (job sites and location points), `pg_stat_statements`.

| Convention | Rule |
| --- | --- |
| Primary keys | `id uuid` UUIDv7 (time-ordered, index-friendly). Generated by the server, or by the agent/mobile app for offline events so retries carry the same ID. |
| Tenant key | `organization_id uuid NOT NULL` on every tenant table, plus `UNIQUE (organization_id, id)` so children can use composite foreign keys. |
| Time | `timestamptz` in UTC. Local calendar facts (`work_date`, shift start in local time) are stored with the IANA timezone used to compute them. |
| Money | `bigint` minor units + `char(3)` ISO currency. Never floats. |
| Durations | `integer` seconds. |
| Enumerations | `text` + `CHECK` constraint (easier to change than PostgreSQL enum types). |
| JSONB | Only for settings, snapshots, evidence and provider payloads — never for data we filter or join on. |
| Soft delete | `deleted_at timestamptz` on user-managed configuration (projects, clients, teams, policies). Not on telemetry (removed by retention jobs) or on ledgers and audit (append-only). |
| Audit classes | **A** = every change written to `audit_logs`. **E** = domain edit history table as well (e.g. `time_entry_edits`). **N** = high-volume telemetry, not audited per row. |
| Optimistic locking | `version integer` on records edited concurrently (time entries, timesheets, timers). Updates send `If-Match`. |
| Partitioning | Monthly range partitions (pg\_partman, 3 months pre-created) on `activity_slots`, `app_usage`, `url_usage`, `screenshots`, `screenshot_access_log`, `location_points`, `device_heartbeats`, `webhook_deliveries`, `outbox_events`, `audit_logs`, `notifications`, `auth_events`. |
| Partitioned tables are leaves | Their primary key includes the partition column, so other tables do not hold foreign keys to them; references are validated by the owning service. |
| Retention | A nightly job deletes rows older than each org's retention in batches of 5,000 via `(organization_id, <time>)` indexes. Partitions older than the longest plan retention are dropped whole. Legal holds pause both. |
| Indexes | Every tenant index starts with `organization_id`. BRIN indexes on time columns of large partitions. Partial indexes for "active" rows (`WHERE deleted_at IS NULL`, `WHERE stopped_at IS NULL`). |
| Integrity | `EXCLUDE USING gist` stops overlapping time entries per member and overlapping rate periods. Only one running timer per member (partial unique index). |
| Migrations | Expand → migrate → contract; `CREATE INDEX CONCURRENTLY`; `lock_timeout = 3s`; no destructive change in the same release as the code that stops using it. |
| Backups | Point-in-time recovery for 35 days; daily snapshots copied to a second region; quarterly restore drill. |

**Volume estimate** (for sizing, to be refined). One member working 8 hours produces 48 activity slots, about 150 screenshot rows (3 per slot, multi-monitor) and 200–400 app/URL rows per day. At 10,000 active members that is about 0.5 M slots, 1.5 M screenshot rows and 3 M usage rows per day — comfortable for partitioned PostgreSQL. The columnar store becomes worthwhile around 100,000 active members (§15).

## 8. Complete database tables

The full catalog — 142 tables with columns, types, keys, indexes, uniques, tenant flag, soft delete and audit class — is in the Database tab.

| Section | Tables | Highlights |
| --- | --- | --- |
| 0. Conventions | — | Implied columns, tags, FK notation |
| 1. Identity & auth | 14 | Sessions, rotating refresh tokens, passkeys, OAuth, auth events |
| 2. Organizations, RBAC, teams | 14 | Members as the tenant identity; policy hierarchy; roles at 3 scopes |
| 3. Projects & tasks | 8 | Rates with no-overlap constraint; budgets |
| 4. Time tracking | 7 | One running timer; no overlapping entries; edit history |
| 5. Activity, apps, websites | 6 | Partitioned slots and usage; global app/domain catalogs |
| 6. Screenshots & devices | 7 | Screenshot metadata, access log, sync batch receipts |
| 7. Attendance & locations | 11 | Shifts, breaks, geofences (PostGIS), GPS points |
| 8. PTO & overtime | 11 | Append-only leave ledger |
| 9. Expenses, payroll, invoicing | 13 | Payments immutable once sent; gap-free invoice numbers |
| 10. Billing | 12 | Provider mirror, seats, entitlements, usage counters |
| 11. Reports | 6 | Daily rollups and invalidation queue |
| 12. Integrations & platform | 11 | Entity maps, outbound webhooks, outbox, idempotency keys |
| 13. Notifications | 6 | Alert rules, inbox, push tokens |
| 14. AI | 5 | Insights, anomalies, NL queries, model versions |
| 15. Audit, privacy, SSO | 11 | Hash-chained audit log, consent, legal holds |

## 9. ERD

The core of the model is small: organizations own members, members own time, and time owns telemetry. Arrows point from the "one" side to the "many" side; every link below is a composite `(organization_id, …)` foreign key except those into partitioned tables.

&#91;embedded content: Core ERD · 14 entities\]

The remaining 128 tables attach to these 14: workforce tables (shifts, breaks, time off, attendance) to `organization_members`; finance tables to `time_entries`, `timesheets` and `projects`; platform tables (audit, notifications, integrations) to `organizations`.

## 10. API architecture

One versioned REST API serves the web app, mobile app, desktop agent and third parties. An OpenAPI 3.1 contract is the source of truth: clients are generated from it and CI fails if the code drifts from it. The full endpoint catalog is in the API tab.

| Surface | Base path | Consumers |
| --- | --- | --- |
| Application API | `/v1/*` | Web, mobile, third-party apps, Zapier, MCP server |
| Agent ingest | `/v1/desktop/*` | Desktop agent and mobile telemetry; served by the Ingest service |
| OAuth / OIDC | `/oauth/*`, `/.well-known/*` | All clients |
| SCIM 2.0 | `/scim/v2/*` | Identity providers (Okta, Entra ID, OneLogin) |
| Inbound provider webhooks | `/hooks/{provider}` | Billing and integration providers |
| Realtime | `wss://realtime.worklume.com` | Web dashboards, desktop (commands) |
| Staff admin | separate host, VPN + SSO | Worklume staff only |

**Conventions** (detailed in the API tab): JSON with snake\_case fields; UTC ISO 8601 timestamps; money as `{amount_minor, currency}`; cursor pagination; RFC 9457 problem+json errors; `Idempotency-Key` on side-effecting POSTs; `ETag` / `If-Match` optimistic locking; `include=` for related resources; per-token and per-org rate limits with `RateLimit-*` headers; `Sunset` and `Deprecation` headers with 12 months' notice.

**Laravel implementation.** Each domain module registers its own route file. A request passes through: TLS at the edge → `AuthenticateToken` → `ResolveTenant` (sets context and `SET LOCAL app.org_id`) → `EnforceEntitlement` (plan features) → rate limiter → FormRequest validation → Policy → application service → API Resource transformer. Controllers never touch Eloquent directly.

**Realtime.** Laravel Reverb with Redis fan-out. Channels: `private-org.{org}.member.{member}` (own timer, notifications, device commands), `private-org.{org}.team.{team}` (team dashboard, who's online), `private-org.{org}.admin` (org-wide events for owners and admins). Events carry IDs and minimal fields; clients refetch details through the API, so permission checks stay in one place.

## 11. Authentication

Worklume is its own OAuth 2.1 / OpenID Connect provider. Every client, first-party included, receives short-lived access tokens and rotating refresh tokens bound to one organization.

| Topic | Design |
| --- | --- |
| Sign-in methods | Email + password (Argon2id; breached-password check via k-anonymity range API); passkeys (WebAuthn, discoverable credentials); email one-time code; SAML 2.0 or OIDC SSO for Enterprise. No social login at launch; Google and Microsoft sign-in can be added as OIDC providers later. |
| MFA | TOTP, passkey, or email code as a fallback; 10 single-use recovery codes. Owners and admins can require MFA for the org (members get 24 h to enrol). |
| Step-up | Sensitive actions need authentication within the last 10 minutes: billing changes, payroll runs, payout accounts, SSO config, screenshot deletion, data exports, API token creation. |
| Access tokens | JWT signed with ES256; 15-minute lifetime. Claims: `sub` (user), `org`, `mid` (member), `sid` (session), `scope`, `aud`, `cnf` (DPoP key thumbprint where bound). Signing keys live in KMS and rotate every 90 days, published at `/.well-known/jwks.json` with `kid`. |
| Refresh tokens | Opaque, stored hashed, rotated on every use. Reuse of an already-rotated token revokes the whole token family and the session. Lifetime 30 days sliding (web, mobile) or 90 days (desktop). |
| Web app | Authorization code + PKCE. Access token held in memory only; refresh token in an `HttpOnly; Secure; SameSite=Strict` cookie scoped to `/v1/auth/refresh`, with a CSRF token check on that endpoint. |
| Desktop agent | Authorization code + PKCE through the system browser with a loopback redirect (RFC 8252). The agent creates a key pair in the OS keychain; tokens are DPoP-bound (RFC 9449), so a stolen refresh token is useless without the device key. |
| Mobile app | Authorization code + PKCE with a claimed HTTPS redirect (universal/app links); DPoP-bound like desktop. |
| Organization switch | `POST /v1/auth/switch-organization` issues new tokens for the target org after checking membership. Old org tokens stay valid until expiry unless revoked. |
| Revocation | Removing a member, changing a password, or an admin "sign out everywhere" revokes sessions. Revoked `sid`s go on a Redis denylist until their access tokens expire (≤ 15 min). |
| SSO | SP-initiated SAML and OIDC; IdP-initiated SAML off by default. JIT provisioning optional. "Require SSO" disables password and passkey sign-in for that org's members (owners keep a break-glass passkey). |
| SCIM | SCIM 2.0 Users and Groups (groups → teams); bearer SCIM token per org; deprovisioning suspends the member and revokes sessions. |
| Third-party apps | OAuth apps request scopes (`time:read`, `time:write`, `reports:read`, `screenshots:read`…); the effective permission is scope ∩ the granting member's permissions. |
| Machine tokens | Personal access tokens (`wlm_pat_`) and organization tokens (`wlm_oat_`), hashed at rest, prefix-identifiable for secret scanning, max 1-year expiry for personal tokens. |

## 12. RBAC

Permissions are fine-grained keys; roles bundle them; role assignments grant a role at organization, team or project scope. Sensitive permissions — screenshots, app/URL detail, pay rates, payroll — are never implied by broader ones and must be granted explicitly.

**Evaluation.** A member's effective permissions for a target = organization role ∪ team roles covering the target member ∪ project roles covering the target project. The resolver caches each member's assignments in Redis (keyed by org and a permissions version that bumps on any change). Policies call `can(permission, target)`; the resolver checks both the permission and the relationship (e.g. is the actor a team lead of the target member's team?).

**Built-in roles**

| Permission area | Owner | Admin | Team Lead | Project Manager | Member | Viewer (free seat) | IT Admin | Billing Admin |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Track own time; see own data, screenshots, activity | ✓ | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ |
| View others' time and activity | All | All | Team | Project | — | Project | — | — |
| View others' screenshots, apps, URLs | All | All | Team (if granted) | Project (if granted) | — | Project (if granted) | — | — |
| Edit others' time; approve manual time | All | All | Team (if granted) | Project | — | — | — | — |
| Approve timesheets and time off | All | All | Team (if granted) | — | — | — | — | — |
| Delete screenshots | All | All (if enabled) | — | Project (if enabled) | Own block (if enabled) | — | — | — |
| Projects, clients, tasks, budgets | All | All | Team projects (if granted) | Project | Tasks on own projects | — | — | — |
| Pay rates, payroll, payments | ✓ | If granted | — | — | Own pay | — | — | — |
| Client invoices, expenses approval | ✓ | ✓ | — | Project expenses | Own expenses | — | — | — |
| Members, teams, roles, policies | ✓ | ✓ (not owners) | — | — | — | — | — | — |
| SSO, SCIM, devices, silent agent, API tokens | ✓ | — | — | — | — | — | ✓ | — |
| Subscription and billing | ✓ | — | — | — | — | — | — | ✓ |
| Audit log | ✓ | ✓ | — | — | — | — | ✓ | — |
| AI insights about others | All | All | Team | Project | Own only | — | — | — |
| Delete or transfer organization | ✓ | — | — | — | — | — | — | — |

"If granted" means the permission is off in the role by default and switched on per assignment. Custom roles are built from the same permission catalog, cannot exceed the creator's own permissions, and cannot include owner-only permissions.

**Field-level rules.** Pay and bill rates are stripped from API responses unless the caller has `rates.view`. Window titles and full URL paths require `activity.view_detail` on top of `activity.view`. An org has at least one owner at all times (enforced in the service and by a deferred constraint).

## 13. Offline synchronization

The desktop agent (and mobile app) treat the local database as the source of truth until the server acknowledges each event. Every event carries a client-generated UUIDv7, so a retry can never create a duplicate, and the server returns a verdict per event.

&#91;embedded content: Offline sync · outbox to acknowledgement\]

**Event envelope** (every outbox row):

| Field | Purpose |
| --- | --- |
| `event_id` | UUIDv7 made on the device; becomes the row ID on the server (slot, screenshot, time entry) |
| `seq` | Monotonic per-device counter; the server tracks the highest contiguous `seq` it has applied |
| `type` | `timer.started`, `timer.stopped`, `slot.closed`, `screenshot.captured`, `app_usage.batch`, `idle.detected`, `idle.resolved`, `break.started`… |
| `wall_time` | Device clock (UTC) when it happened |
| `mono_time` | Monotonic clock reading and boot ID — immune to clock changes |
| `clock_offset_ms` | Last known device-minus-server offset |
| `payload` | Event data; screenshot bytes are referenced, not embedded |

**How each failure is handled**

| Situation | Handling |
| --- | --- |
| Offline | Tracking continues; events accumulate locally (cap: 30 days or 2 GB, then oldest screenshots are dropped first with a warning to the user and admin). Sync resumes on reconnect. |
| Retry | Exponential backoff with full jitter (1 s → 5 min cap). A batch keeps its `batch_id`; the server replays the stored response from `sync_batches`. |
| Duplicate events | Server inserts use the client `event_id` as the key (`ON CONFLICT DO NOTHING`) and report `duplicate`; the agent marks it sent. |
| Conflicts | Server rules win, and the verdict tells the agent what happened. (1) Time on a locked timesheet → `rejected: locked`, surfaced to the member as a manual-time request. (2) Overlap with an existing entry → the later-arriving interval is trimmed to the gap; the trimmed part is logged in `time_entry_edits`. (3) Project archived or membership removed while offline → accepted into the "Unassigned" state for a manager to fix. (4) Timer started on two devices → the earlier start wins; the other device is told to stop (see below). |
| Multi-device | One running timer per member, enforced by the partial unique index. Starting on device B while A runs sends A a `stop_timer` command over WebSocket (or on its next sync); A's time up to the switch is kept. |
| Device sleep | `PowerEvents` trait reports sleep/wake. Sleep closes the current slot and pauses the timer's clock; on wake, the gap is recorded as not tracked. A sleep longer than the idle timeout triggers the idle dialog. |
| Crash | Every state change is journaled before it takes effect (SQLite WAL). On restart the agent replays the journal, closes a half-open slot using the last heartbeat time, and continues. The watchdog restarts the agent within 5 s. |
| Clock changes | Durations come from the monotonic clock, never from wall-clock subtraction. The server corrects `wall_time` with `clock_offset_ms`; if the corrected time differs from the server's receive time by more than 5 min for a live event, the event is flagged `clock_adjusted` and anchored to monotonic deltas from the last trusted sync. Deliberate clock manipulation surfaces as an anomaly. |
| Expired authentication | 401 → refresh the access token and retry. If the refresh token is dead or revoked, the agent keeps recording locally for up to 24 h, shows a sign-in prompt, and uploads after re-login. If the member was removed or the device revoked, local data for that org is uploaded only if the server still accepts the device; otherwise it is wiped after 7 days. |
| Server-side limit reached | Weekly limit or budget stop → `rejected: limit` with the cut-off time; the agent stops the timer and trims local data to match. |

**Server ingest path.** `POST /v1/desktop/sync` accepts up to 500 events or 5 MB. The Ingest service validates the batch, writes the `sync_batches` receipt, applies events in `seq` order inside one transaction per member, emits outbox events, enqueues `rollup_invalidations`, and returns per-event verdicts plus the highest applied `seq`. Target p95: under 300 ms for a normal 10-minute batch.

## 14. Screenshot architecture

Screenshot bytes never pass through the API servers. The agent uploads straight to S3 with a single-use presigned URL that the server issues only after checking policy, entitlements and quota. A worker then verifies and finishes each image. Deletion is real: the bucket is not versioned.

**Pipeline**

1. **Capture (desktop).** At a random second inside each 10-minute slot, up to the policy count (1–3, more with the add-on). Skipped while idle, paused or on break. All monitors, or the active monitor only if the org chooses. If the OS denies capture, the agent records `capability: denied` instead of a gap.
2. **Compress (desktop).** Blur first if the policy requires it (irreversible). Downscale to a longest edge of 1,920 px, encode as WebP at quality 70, compute SHA-256.
3. **Encrypt (desktop).** The file is written to the spool encrypted with AES-256-GCM, key from the OS keychain. It is decrypted only in memory for upload; transport is TLS 1.3.
4. **Queue (desktop).** A `screenshot.captured` outbox event records ID, slot, monitor, size and hash. The file stays in the spool until the server confirms.
5. **Presigned upload.** The agent calls `POST /v1/desktop/screenshots/upload-urls` with up to 20 items. The server checks the device, the member's active status, the slot's allowed count and the plan quota. It creates `screenshots` rows (`pending_upload`) and returns presigned PUT URLs: 5-minute expiry, exact `Content-Length`, required `x-amz-checksum-sha256`, SSE-KMS and a retention tag. The S3 key is computed by the server.
6. **API metadata.** After a successful PUT the agent sends `screenshot.uploaded` in its next sync batch. An S3 `ObjectCreated` event to SQS confirms it independently, covering an agent crash between upload and ack.
7. **Processing (worker).** HEAD the object; compare size and checksum; decode to prove it is an image; strip metadata; generate a 320 px thumbnail; set `status = ready`; increment the usage counter; publish `ScreenshotReady` to the team channel. Failures go to `failed` and a dead-letter queue. No OCR or content analysis is done by default.
8. **Dashboard.** The API returns metadata only. The browser loads images through presigned GET URLs valid for 60 seconds, issued per image after a permission check. Views are written to `screenshot_access_log` (batched).

**Storage layout.** Bucket `worklume-{env}-{region}-screenshots`: Block Public Access on, no versioning, bucket policy requiring TLS and SSE-KMS, access only via the app's IAM role. Key `orgs/{org}/screenshots/{yyyy}/{mm}/{member}/{id}.webp` plus `…/{id}.thumb.webp`. KMS encryption context `{org_id}`.

**Retention**

| Layer | Mechanism |
| --- | --- |
| Policy | Org sets retention ≤ its plan maximum (e.g. 14, 30, 90, 180 days; up to 365 with the add-on). `retention_until` is stamped at ingest. |
| Nightly job | Deletes expired objects with batched S3 `DeleteObjects` and marks rows `expired`, per org, logged in `retention_runs`. |
| Backstop | Objects carry a tag `ret=<days>`; S3 lifecycle rules expire each tag class 7 days after its maximum, so nothing survives even if the job fails. |
| Legal hold | Held objects are re-tagged `ret=hold` (no lifecycle rule) and skipped by the job until released. |
| Plan downgrade | New retention applies to new captures; existing objects keep their stamped date. |
| Org cancellation | All screenshots deleted at `purge_after` (90 days after cancellation). |

**Access control**

| Who | Can |
| --- | --- |
| The member | Always see their own screenshots (in web, mobile and the agent's "my data" view). Delete their own activity block (screenshot + time) only if the org allows it. |
| Team lead / project manager | View only with the explicit `screenshots.view` grant, only for their team or project. |
| Admin / owner | View all; delete a screenshot but keep the time (if the org enables deletion); step-up MFA for bulk download. |
| Worklume staff | No access except via an audited break-glass grant. |

**Storage estimate** (assumptions, for cost modelling): about 150 KB per image, 3 per slot, one monitor, 8 h/day, 22 days/month ≈ 475 MB per member per month. At 180-day retention that is roughly 2.9 GB stored per member, about US$0.07 per member per month on S3 Standard list prices, before request and transfer costs.

## 15. Reporting architecture

Start with PostgreSQL rollup tables on a read replica. Add a columnar analytics store only when measured thresholds are crossed. A warehouse is for Worklume's own BI and AI, not for customer reports.

| Option | Use it for | When it becomes necessary |
| --- | --- | --- |
| PostgreSQL base tables | Detail views (a member's time entries, a timesheet), payroll and invoice calculations, anything that must be exactly current | Day one; money is always computed from base tables at approval time, never from rollups |
| Rollup tables (`rollup_member_daily`, `rollup_usage_daily`) | All summary reports, dashboards, budgets, utilization | Day one. Workers recompute a (member, day) key within about 60 s of any change via `rollup_invalidations` |
| Read replica | Report queries and exports | Day one; one replica. Pages that must read their own writes use the primary |
| Materialized views | Platform-wide internal stats only | Rarely. `REFRESH` recomputes every tenant at once, so they don't suit per-tenant, incremental reporting |
| Analytics DB (ClickHouse) | Insights, long-range trends, benchmarks across tenants, ad-hoc slicing of app/URL data | When any holds: report p95 > 2 s on rollups; > \~100k active members; > \~1 B slot rows; benchmarks launch. Fed by CDC from PostgreSQL; `organization_id` leads every sort key and row policies enforce tenant scope |
| Warehouse / lake (S3 + Athena, or Snowflake) | Worklume's own BI, finance, product analytics, anonymised AI training data | After product-market fit, when internal analysts need it. Never serves customer requests |

**Report engine.** Each report type is a typed query class with declared filters, columns and permissions, used by the UI, exports, schedules, the public API and the AI's natural-language layer alike. Exports run as jobs: rows are streamed with a server-side cursor to CSV or XLSX (OpenSpout) or rendered to PDF by a headless-Chromium service (Gotenberg), stored in S3 for 7 days and linked by presigned URL. All dates are in the org timezone unless the request says otherwise.

## 16. Billing architecture

A payment provider runs charges, tax and dunning. Worklume owns seats, entitlements and usage. A `BillingProvider` interface keeps the provider swappable.

| Topic | Design |
| --- | --- |
| Provider | Paddle (merchant of record: handles global VAT/GST, invoices, chargebacks) or Stripe Billing + Stripe Tax. The choice depends on Worklume's legal entity and markets: an India-based entity selling globally favours a merchant of record. Requires validation (ADR-007). |
| Seats | Every active member except Viewers is billable. Each change writes `seat_events`; a job syncs the quantity to the provider (increases immediately, decreases at renewal). |
| Proration | Seat additions are prorated for the rest of the term on every interval. Removals take effect at renewal with no refund. Upgrades apply immediately and are prorated; downgrades are scheduled for period end (`pending_change`). These rules are published on the pricing page. |
| Trials | 14 days, no card, full Team-plan entitlements; expiry moves the org to `suspended` (read-only) until a plan is chosen. |
| Entitlements | Plan features + add-ons + overrides resolved into `entitlements`, cached per org; `EnforceEntitlement` middleware and the policy engine read them; agents receive them in their config. |
| Usage limits | Screenshot and app/URL caps on lower plans are counted in Redis per member per month and flushed to `usage_counters`; at the cap, the upload-URL endpoint returns `quota_exceeded` and the agent stops capturing. |
| Dunning | Provider retries for 7 days (`past_due`, full access, banner). Then `suspended`: read-only, agents stop tracking, data kept. Cancelled orgs are purged 90 days after cancellation. |
| Events | Provider webhooks land in `billing_webhook_events` (unique on provider event ID), are processed once, and update subscriptions, invoices and entitlements. |
| Invoices | The provider's invoices are mirrored in `billing_invoices` for the in-app billing page. |

## 17. Integration architecture

Every third-party connector implements one `Connector` contract; a shared sync engine handles scheduling, retries, rate limits and loop prevention.

| Part | Design |
| --- | --- |
| Connector contract | `authorize()`, `refreshCredentials()`, `listProjects()`, `pullTasks(cursor)`, `pullUsers()`, `pushTime(entry)`, `pushTaskStatus()`, `handleWebhook(event)`, `capabilities()` |
| Inbound | Provider webhooks where offered (Jira, GitHub, GitLab, ClickUp, Asana) land in `integration_inbound_events` and are processed by jobs; otherwise incremental polling every 5 min per linked project using `sync_cursor` |
| Outbound write-back | Triggered by `time_entry.*` outbox events. Modes per connection: hourly, daily, delayed (24 h, so edits settle), on task completion, or off. Time is written as worklogs or comments depending on the provider |
| Loop prevention | `integration_entity_maps` stores the external ID, version and content hash; an update whose hash matches the last synced state is skipped |
| Limits and failures | Per-connection Redis token bucket sized to each provider's limits; exponential backoff; a circuit breaker sets `status = error` after repeated failures; `needs_reauth` notifies the connecting member and admins |
| Credentials | Envelope-encrypted with KMS (encryption context: org and connection); refreshed by scheduled jobs before expiry |
| Fairness | Integration jobs run on their own queue, rate-limited per org |
| Customer webhooks | Events from the outbox are delivered to `webhook_endpoints`, signed `Worklume-Signature: t=<ts>,v1=<HMAC-SHA256>`, retried 8 times over 24 h, disabled after 50 consecutive failures, with a replay endpoint. Includes time-entry update and delete events (a gap in Hubstaff) |
| Launch set | V1: Jira, Asana, ClickUp, GitHub, Slack, Zapier. V2: QuickBooks, Xero, BambooHR, Google/Microsoft calendar, payouts (Wise, PayPal, Deel, Gusto). Later: the long tail |
| MCP server | A read-only MCP server over the public API, OAuth-authorised, scoped like any third-party app (Future phase) |

## 18. AI architecture

AI runs in a separate plane that sees derived data only and writes back through the same internal APIs as any other client. Numbers come from deterministic code; the LLM only explains, phrases and plans queries. No AI output changes pay, time or status without a human action.

**Separation from transactional data**

| Boundary | Rule |
| --- | --- |
| Accounts and network | The AI service runs in its own AWS account and VPC. It has no credentials for the primary database. |
| Data in | CDC/outbox events and nightly exports of rollups land in the analytics store (a PostgreSQL analytics instance at first, ClickHouse later). The AI service reads only from there. |
| Data out | Results are posted to internal endpoints (`/internal/ai/insights`, `/internal/ai/anomalies`) that validate, apply tenant context and write `ai_*` tables. |
| LLM provider | Amazon Bedrock (Claude models) in the org's data region: no training on customer data, no provider-side retention. Per-org opt-in switch and token quota. |

**Data tiers**

| Tier | Examples | May reach an LLM? |
| --- | --- | --- |
| 0 — Aggregates | Hours per day, activity %, focus minutes, utilization, budget burn | Yes |
| 1 — Labels | App and domain names, project names, classification ratings | Yes, if the org enables it |
| 2 — Raw content | Screenshots, window titles, URL paths, GPS points, notes | Never. Used only by deterministic detectors inside the plane |

Keeping tier 2 out of prompts also closes the main prompt-injection path: a window title or page path could otherwise carry instructions to the model.

**Capabilities**

| Capability | How it works | Output |
| --- | --- | --- |
| Anomaly detection | Versioned rules (constant activity, keyboard without mouse, suspicious apps, clock tampering) plus per-member statistical baselines (z-scores on activity variance and input ratios). The agent computes input-timing features on the device (e.g. entropy of intervals between inputs) and sends only those numbers, so automated input can be detected without logging keystrokes. Runs nightly; suspicious-app hits run near real time. | `ai_anomalies` with confidence and a plain explanation; a reviewer confirms or dismisses; nothing is penalised automatically |
| Productivity summaries | A nightly job builds a facts JSON per member and team from rollups: hours, projects, focus, classification split, change against the member's own baseline. A fixed prompt template turns facts into 3–5 sentences. A validator rejects any output containing a number not present in the facts. | `ai_insights` (daily/weekly summary); members see their own, managers their scope |
| Natural-language reporting | The model is given tools that map to report query classes (metrics × dimensions × filters) — never raw SQL. Its plan is validated against the schema and the asker's permissions, executed by the Reports domain under the asker's tenant context, then summarised with links to the underlying report. Questions about people the asker cannot see are refused. | `ai_queries` with the executed plan as citation |
| Workload analysis | Deterministic: utilization vs expected hours, overtime trend, meeting load, focus erosion, budget burn forecast. The LLM only explains. | Workload insights per team and member |
| Recommendations | Rules generate candidates ("Project X will exceed budget in about 9 days", "3 members above 110% utilization for 3 weeks", "40% of app time is unclassified"), ranked by impact, phrased by the LLM, each with evidence and a one-click action. Feedback tunes ranking. | `ai_insights` of type recommendation |
| Classification suggestions | For unclassified apps and domains, suggest a rating per job type from the global catalog and model knowledge; an admin approves. | `productivity_classifications` with `source = ai_suggested` |

**Quality and governance.** Every output stores its model and prompt version (`ai_model_versions`). New versions run in shadow mode against golden datasets before activation. No emotion, sentiment or personality inference — the EU AI Act prohibits emotion recognition in the workplace and treats AI used to monitor or evaluate workers as high-risk, so Worklume positions AI as decision support with human oversight, documentation and employee visibility of insights about themselves.

## 19. Security architecture

Security rests on four things: tenant isolation (§4), short-lived device-bound tokens (§11), envelope encryption with KMS, and an audit trail customers can see. Authentication, MFA, passkeys, OAuth, SSO and RBAC are covered in §11–12.

| Area | Controls |
| --- | --- |
| API security | TLS 1.2+ (1.3 preferred) with HSTS at CloudFront; AWS WAF managed rules plus rate rules; strict CORS (the app origin only); FormRequest validation on every input; mass-assignment protection; UUID IDs only (not enumerable); 404 for foreign IDs; request size and JSON depth limits; SSRF protection for customer webhooks (resolve and block private and link-local ranges, send through an egress proxy); security headers and CSP on the web app. |
| Encryption in transit | TLS everywhere, including inside the VPC to RDS, Redis and internal services. |
| Encryption at rest | RDS, ElastiCache, EBS and S3 encrypted with KMS customer-managed keys. Application-level envelope encryption for secrets and sensitive fields: TOTP seeds, integration credentials, webhook secrets, payout references, window titles. Each org has a data key wrapped by the environment CMK with encryption context `{org_id}`; annual CMK rotation. |
| S3 security | Separate buckets per data class (screenshots, receipts, exports, payslips, audit anchors with Object Lock, public agent releases behind CloudFront). Block Public Access on all private buckets; policies deny non-TLS and non-KMS writes; the app role may only touch the `orgs/` prefixes; presigned URLs are single-object and 60–300 s; access logs go to the log-archive account. |
| Desktop security | Signed and notarized builds; update packages signed (Tauri updater keys) and checked before install; SQLCipher local store with the key in the OS keychain; DPoP-bound tokens; Tauri capability allowlist for IPC; strict CSP and no remote content in the webview; the agent needs no admin rights (except the silent MDM install); tamper signals (killed process, clock jumps, blocked uploads) reported as heartbeat gaps. |
| Token rotation | Refresh tokens rotate on every use with reuse detection; JWT signing keys rotate every 90 days; database and service credentials in Secrets Manager rotate every 30 days; webhook secrets rotate with a 24 h overlap; integration tokens refresh before expiry; personal access tokens expire within a year. |
| Rate limiting | Edge: WAF per-IP limits. App: token bucket per token (default 600 requests/min), per org (10,000/min), per device for ingest (60 batches/min). Auth: 5 failed logins per account per 15 min, then progressive delay and email-code challenge. Expensive endpoints: exports 10/hour per member; AI queries by plan quota. Responses carry `RateLimit-*` and `Retry-After`. |
| Audit logs | Every admin, permission, policy, billing, time-edit, approval, screenshot view and delete, export, token, SSO, device and break-glass event goes to `audit_logs` (hash-chained, insert-only role, daily anchor in an Object Lock bucket). Customers see and export it; Enterprise can stream it to their SIEM. |
| Infrastructure | AWS Organizations with separate accounts (prod, staging, dev, security/log archive, shared services, AI). Private subnets only for compute and data; no SSH (SSM Session Manager); least-privilege IAM per service; GuardDuty, Security Hub, AWS Config and an organization CloudTrail. |
| Secure development | PHPStan at max level, Semgrep, Clippy, ESLint; dependency scanning (Composer, npm, Cargo); secret scanning with token prefixes; DAST against staging; external penetration test before launch and yearly. |
| Compliance path | SOC 2 Type I around V1, Type II six months later; ISO 27001 afterwards. Incident response runbooks with customer notification within 24 h of a confirmed breach. |

## 20. Privacy architecture

Worklume is designed so that the employee always knows what is collected and can see it, and so that the least invasive setting is the default. This is both a legal requirement in many markets and a product advantage over Hubstaff.

| Principle | How the product enforces it |
| --- | --- |
| Notice before tracking | A monitoring notice is generated from the org's actual policies (what, when, who can see it, retention). A member must acknowledge it before the first tracking session; every policy change creates a new version and asks again (`monitoring_notices`, `consent_records`). |
| Visible tracking | The tray icon and status are always visible while tracking. Silent mode is Enterprise-only and needs the org to confirm a lawful basis and a published notice. |
| Track only work time | By default nothing is collected while the timer is stopped. "Always-on" automatic tracking and "always" GPS mode are opt-in policies that the notice must disclose. |
| Minimal defaults | New orgs start with screenshots off, URL tracking domain-only, window titles off, GPS off. Keystrokes are never recorded; only active/inactive seconds. |
| Employee access | Members see their own screenshots, activity, apps, URLs, location history and AI insights, and can request an export at any time. |
| Aggregate-only mode | For works-council or stricter regimes, an org can disable individual-level activity, app and screenshot views so managers see team aggregates only. |
| Retention by data class | Defaults: screenshots 30 days, app/URL 30 days, GPS 30 days, activity slots and time 3 years (payroll law may require longer), audit 2 years. Orgs can shorten any of them; legal holds suspend deletion. |
| Data subject rights | Access and export (JSON, CSV and a screenshot ZIP), rectification, and erasure. On erasure, records the employer must keep for payroll or tax are pseudonymised instead of deleted, and deleted when the legal period ends. |
| Data residency | Each org is pinned to a regional cell at signup (for example India, US, EU). Tenant data and backups stay in that jurisdiction. |
| Roles under data law | The customer is the data controller for its employees' data; Worklume is the processor under a DPA with a published sub-processor list. |
| Legal mapping | GDPR (customers get a DPIA template; legitimate-interest balancing notes), India DPDP Act 2023, CCPA/CPRA for employee data, and works-council requirements via aggregate-only mode. The exact obligations per market need legal review before launch. |
| AI | Tier rules from §18; members see AI insights about themselves; orgs can switch AI off entirely. |

## 21. Infrastructure architecture

One AWS "cell" per data region, each a complete copy of the stack, defined in Terraform. Launch with a single cell in the first target market; add cells for data residency, not for scale.

| Layer | Services |
| --- | --- |
| Accounts | AWS Organizations: management, security/log-archive, shared services (CI runners, ECR), prod cell(s), staging, dev, AI. |
| Network | VPC across 3 AZs; public subnets (ALB, NAT); private app subnets (ECS); isolated data subnets (RDS, ElastiCache); VPC endpoints for S3, SQS, KMS, Secrets Manager and ECR. |
| Edge | Route 53, CloudFront (web SPA from S3, API pass-through, agent downloads), AWS WAF, ACM certificates. |
| Compute | ECS Fargate on Graviton (arm64). Services: `api`, `ingest`, `realtime` (Reverb), `worker` (Horizon; queues: default, ingest, screenshots, reports, integrations, notifications, billing, ai-export), `scheduler` (one task), `pdf` (Gotenberg). Autoscaling on CPU, request count and queue depth. |
| Data | RDS PostgreSQL 17 Multi-AZ + one read replica; ElastiCache (Valkey) for cache, queues, locks and Reverb pub/sub; S3; SQS with dead-letter queues; KMS; Secrets Manager. |
| Messaging | SES for email; APNs and FCM for push. |
| AI account | Python services on Fargate, analytics store, Bedrock access; connected to the prod cell only through internal endpoints and the export pipeline. |
| Observability | OpenTelemetry traces and metrics; structured JSON logs tagged `org_id` and `request_id`; Sentry for PHP, web, desktop and mobile errors; Grafana dashboards; PagerDuty (or similar) on-call. |
| CI/CD | GitHub Actions; monorepo `apps/api`, `apps/web`, `apps/mobile`, `apps/desktop`, `packages/*`; Terraform plans reviewed in PRs; migrations run as a pre-deploy ECS task; rolling or blue/green ECS deploys; feature flags for risky changes; desktop builds signed in CI on macOS and Windows runners. |
| Disaster recovery | RPO ≤ 5 minutes (PITR), RTO ≤ 4 hours; encrypted backups copied to a second region in the same jurisdiction; quarterly restore drill. |

**Service-level objectives (initial):** API availability 99.9% monthly; ingest p95 < 300 ms; dashboard p95 < 1 s; report p95 < 2 s; screenshot visible on the dashboard within 2 minutes (p95) of upload.

## 22. Scalability architecture

The stack scales horizontally everywhere except PostgreSQL, so the plan concentrates on keeping the database's hot path small and on moving telemetry out when it grows.

**Load model.** Each tracking member sends one sync batch every 10 minutes (plus screenshot uploads straight to S3). That is about 17 batches per second at 10,000 concurrent members and 170 per second at 100,000. Device heartbeats (every 60 s) go to Redis only and are sampled to PostgreSQL every 5 minutes, so they never become the bottleneck.

| Stage | Active members | What changes |
| --- | --- | --- |
| A | up to \~5,000 | Single cell; one RDS primary (db.r7g.large class) + replica; 2–3 tasks per service |
| B | \~5,000–50,000 | PgBouncer in transaction mode; separate Redis clusters for cache and queues; more replicas for reports; ingest autoscaling on queue depth |
| C | \~50,000–250,000 | Move telemetry tables (`activity_slots`, `app_usage`, `url_usage`, `screenshots`, `location_points`) to a dedicated PostgreSQL cluster — possible because they are leaf tables with soft references; ClickHouse for analytics and Insights; SQS buffer in front of ingest |
| D | 250,000+ | Shard the telemetry cluster by `organization_id` (e.g. Citus) or add cells; dedicated cells for very large tenants |

**Other levers**

- **Noisy neighbours:** per-org rate limits and per-org job concurrency caps; a very large tenant can be moved to its own cell.
- **WebSockets:** Reverb nodes scale horizontally behind the ALB with Redis pub/sub; clients reconnect with backoff.
- **Rollups** keep report cost proportional to members × days, not to raw rows.
- **Partitions** keep indexes small and make retention a cheap `DROP`.
- **Octane** keeps PHP workers warm; per-request state is reset explicitly (see risk R3).

## 23. Technology decisions

I accepted most of the proposed stack, changed one choice (Next.js for the app) and added four that the brief left open. Each choice is weighed against its realistic alternatives below.

| Area | Proposed | Recommendation | Alternatives considered | Why / trade-off |
| --- | --- | --- | --- | --- |
| Web app | React + TS + Next.js | **React + TS + Vite SPA** (TanStack Router/Query/Table, Tailwind + Radix UI) for the logged-in app; **Next.js (static export)** for marketing and docs | Next.js App Router for everything; React Router 7; Inertia.js on Laravel | The app sits behind login and talks to a separate API, so server rendering buys little and adds a Node tier. Next.js still wins for SEO pages. Inertia would couple the web UI to Laravel while mobile, desktop and public clients need the same API anyway. |
| Backend | Laravel, PHP, REST | **Accept:** Laravel 12, PHP 8.4, Octane (FrankenPHP), modular monolith | NestJS (TypeScript everywhere); Go; Django | Your strongest stack; first-party Horizon, Reverb, Pennant and Passport; fast delivery. Trade-off: long-running Octane workers need discipline about static state; the ingest hot path can move to Go or Rust later if profiling demands it. |
| API style | REST | **Accept:** REST + OpenAPI 3.1, contract-first | GraphQL; gRPC | REST fits public API, Zapier and caching; GraphQL would help dashboards but complicates per-field authorization and rate limiting. |
| Database | PostgreSQL | **Accept:** RDS PostgreSQL 17 | MySQL 8; Aurora; CockroachDB | PostgreSQL gives RLS, exclusion constraints, native partitioning, PostGIS and JSONB — all used here. MySQL (your familiar option) lacks RLS and exclusion constraints. |
| Cache and queues | Redis, queues | **Accept:** ElastiCache Valkey + Horizon; **add** SQS for S3 events and as an ingest buffer at scale | SQS only; RabbitMQ; Kafka | Horizon gives visibility and per-queue control; SQS gives durability for events S3 emits. Kafka is only worth it with CDC to ClickHouse (Stage C). |
| Storage | S3 | **Accept** | — | Presigned uploads, lifecycle rules, Object Lock and KMS cover every need. |
| Realtime | WebSockets | **Laravel Reverb** | Pusher or Ably (managed); API Gateway WebSockets | Self-hosted and cheap at scale; Ably is the fallback if operating it becomes a burden. |
| Desktop | Tauri + Rust + React/TS | **Accept:** Tauri 2 | Electron; native per OS (Swift, C#, GTK); Qt | Small footprint, Rust suits OS APIs, one codebase. Trade-offs: WebKitGTK quirks on Linux, tray on GNOME, and the team needs Rust skills. Electron would still need native modules for capture and input. |
| Mobile | React Native | **Accept:** React Native with Expo dev builds | Flutter; native Swift/Kotlin | Shares TypeScript, API client and schemas with web; background location needs native modules either way. |
| Cloud | AWS | **Accept:** AWS, ECS Fargate | EKS; Laravel Cloud or Vapor; Hetzner/DigitalOcean | KMS, Bedrock, compliance services and Indian/EU/US regions. Fargate avoids running Kubernetes. Vapor (Lambda) suits neither Octane, Reverb nor long jobs. |
| Auth | — | **Own OAuth 2.1/OIDC server** (league/oauth2-server via Passport, extended for DPoP); evaluate **WorkOS** for SAML and SCIM | Auth0, Cognito, Clerk | Core auth is too central to rent; enterprise SSO/SCIM is months of edge cases that a vendor can shortcut. |
| Billing | — | **Paddle or Stripe** behind an interface (ADR-007) | Chargebee; Razorpay (India-only) | Depends on legal entity and tax exposure. |
| Analytics | — | Rollups now; **ClickHouse** at Stage C | Redshift; TimescaleDB; Snowflake | ClickHouse is fastest per dollar for event-style aggregates. |
| AI | — | **Python service + Amazon Bedrock (Claude)** | OpenAI API; self-hosted models | Data stays in-region under AWS terms; Python has the best ML tooling. |
| IaC and observability | — | Terraform; OpenTelemetry; Sentry; Grafana | Pulumi; Datadog | Portable and cost-controlled. |

## 24. ADRs

Architecture decision records, all with status **Proposed** until you accept them. Each will get its own page once work starts.

| ADR | Decision | Main alternative rejected | Consequence to accept |
| --- | --- | --- | --- |
| ADR-001 | Laravel modular monolith with enforced module boundaries | Microservices | One deployable; boundaries enforced by architecture tests, not the network |
| ADR-002 | Pooled multi-tenancy: shared schema, `organization_id`, RLS, composite FKs | Schema or database per tenant | Every table and query must follow the tenant conventions; CI enforces them |
| ADR-003 | Tokens bound to one organization; no org ID from clients | Org ID in the URL path | Switching org mints new tokens; multi-org clients hold one token per org |
| ADR-004 | UUIDv7 everywhere; client-generated for offline events | bigint identity | Larger keys; natural idempotency |
| ADR-005 | Monthly partitions for telemetry; partitioned tables are FK leaves | Unpartitioned tables | Soft references validated in services; cheap retention and future split-out |
| ADR-006 | Desktop agent in Tauri 2 with a Rust core and per-OS trait adapters | Electron; three native apps | Rust hiring or up-skilling; one tracking engine |
| ADR-007 | Billing through a provider behind `BillingProvider`; merchant of record if the entity is India-based | Building billing in-house | Provider fees; pricing rules must fit the provider's model |
| ADR-008 | Screenshots uploaded directly to S3 with presigned URLs; no bucket versioning | Upload through the API | Deletes are final; API servers never carry image bytes |
| ADR-009 | Reporting from daily rollups on a replica; ClickHouse at Stage C | Warehouse from day one | Rollup correctness must be tested like business logic |
| ADR-010 | AI in a separate plane with tiered data; LLM never sees raw content | LLM over the transactional DB | Fewer AI features (no screenshot understanding) in exchange for privacy |
| ADR-011 | Own OAuth/OIDC with DPoP-bound tokens for desktop and mobile | Hosted identity provider for everything | More security engineering; full control of tokens |
| ADR-012 | Transactional outbox for all side effects (webhooks, realtime, integrations, AI) | Dispatching events directly from code | One extra table and relay; no lost or phantom events |
| ADR-013 | Web app as a Vite SPA; Next.js only for marketing | Next.js for the app | Two front-end build setups |
| ADR-014 | Privacy-first defaults (screenshots off, domain-only URLs, notice before tracking) | Hubstaff-like defaults | Slightly more onboarding for admins; stronger legal and brand position |
| ADR-015 | One AWS cell per data region; orgs pinned at signup | Single global region | Region moves need an export/import tool |

## 25. Technical risks

These are the architecture-level risks. The product and OS risks from Phase 1 (macOS permissions, Wayland, URL capture, payroll, privacy law) still apply and are repeated only where the architecture changes them. Three need action before any production code: R1, R3 and R6.

| # | Risk | Why it matters | Likelihood / impact | Mitigation |
| --- | --- | --- | --- | --- |
| R1 | Cross-tenant data leak | Existential for a monitoring product | Low / critical | Four enforcement layers (§4), cross-tenant test suite in CI, RLS fail-closed, external pen test focused on isolation |
| R2 | RLS performance or complexity | Policies on every query; mistakes cause empty results or slow plans | Medium / medium | `organization_id` leads every index; benchmark RLS on the largest tables early; keep policies to a single equality check |
| R3 | Octane state leaking between requests | A tenant context or cached user from request A visible in request B | Medium / critical | Tenant context as a scoped (per-request) binding; Octane `RequestTerminated` resets; a test that alternates two orgs' requests on one worker; `SET LOCAL` scoped to each transaction |
| R4 | Rust and Tauri skills gap | The desktop agent is the product; the team's strength is PHP | High / high | Hire or contract one senior Rust engineer; keep the webview UI thin; spike each OS adapter first |
| R5 | Tauri platform gaps (WebKitGTK, GNOME tray, updater on Linux) | Linux quality suffers | Medium / medium | Test on Ubuntu 22.04 and 24.04 in CI; AppIndicator dependency declared; own apt repository for updates |
| R6 | Offline conflict rules wrong | Lost or double-counted time; payroll disputes | Medium / high | Write the conflict rules as a spec with property-based tests (random offline sequences, clock jumps, device switches) before building ingest |
| R7 | Rollups drifting from base data | Reports disagree with timesheets and invoices | Medium / high | Nightly reconciliation job compares rollups with base tables per org/day; money always computed from base tables |
| R8 | Partition maintenance failure | Inserts fail when a month has no partition | Low / high | pg\_partman creates 3 months ahead; an alert fires if fewer than 2 future partitions exist; a default partition as a safety net |
| R9 | Screenshot storage cost | S3 cost grows with retention × monitors × frequency | Medium / medium | WebP compression, retention caps per plan, lifecycle backstop, cost per seat on the ops dashboard |
| R10 | Billing provider fit for an India-based entity | Blocks revenue at launch | Medium / high | Confirm entity and provider eligibility now; keep `BillingProvider` abstraction; Razorpay as an India-only fallback |
| R11 | Enterprise SSO/SCIM effort | Months of IdP edge cases delay enterprise deals | High / medium | Evaluate WorkOS early; build only the SAML/OIDC core in-house if the cost is wrong |
| R12 | AI regulation and trust | EU AI Act high-risk classification for worker monitoring; employee backlash | Medium / high | Tiered data, no emotion inference, human review, employee visibility, per-org off switch, legal review before EU launch |
| R13 | Integration churn | 30+ third-party APIs change and fail | High / medium | Connector contract, per-connector contract tests, circuit breakers, launch with 6 connectors |
| R14 | Single-region outage | All tenants in a cell down | Low / high | Multi-AZ everywhere; cross-region backups; documented 4 h RTO; status page |
| R15 | Data-residency migration | Moving an org between cells is hard later | Low / medium | Org-scoped export/import tool designed now (the same one used for data-subject exports) |
| R16 | Clock manipulation and fraud | Inflated hours | Medium / medium | Monotonic durations, server offset correction, anomaly rules, heartbeat gap reporting |

**Before Phase 3 (build)**, I recommend five spikes: (1) the RLS + Octane tenant-context test harness; (2) the offline sync conflict spec with property tests; (3) Tauri agent spikes on macOS, Windows and Ubuntu Wayland; (4) billing provider confirmation for your legal entity; (5) a cost model for screenshots and ingest at 1,000 and 10,000 members.
