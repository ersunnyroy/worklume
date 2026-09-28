# Worklume — Phase 3 Implementation Plan

Sep 28, 2026 · @Suni Kumar

This plan turns the Phase 1 research and the Phase 2 architecture into
36 epics, 262 stories and 157 ordered migrations, sized XS–XXL rather
than in days. The main tab holds the sequence, plans and release scope;
the other tabs hold the full epic/story detail, the migration list and
the master backlog.

## 1. Development sequence

The proposed order is broadly right, with six changes forced by the
architecture. The biggest: the desktop agent starts on day one as a
parallel track, because it is the product's longest and riskiest path.

<img src="media/image1.png" style="width:6in;height:6.40178in"
alt="Development sequence · 9 stages, 3 release gates" />

Development sequence · 9 stages, 3 release gates

| \#  | Change to the proposed order                                                                     | Why                                                                                                           |
|-----|--------------------------------------------------------------------------------------------------|---------------------------------------------------------------------------------------------------------------|
| 1   | Multi-tenancy is built **with** organizations, directly after authentication                     | RLS, composite keys and TenantContext need the organizations table, and no tenant table may exist before them |
| 2   | A "platform kernel" (outbox, audit log, idempotency, error format, jobs) comes before any domain | Every domain writes audit rows and outbox events; retrofitting them is expensive                              |
| 3   | Policies, entitlements and the privacy notice come before time tracking                          | The agent cannot track until it has a config and the member has acknowledged the notice                       |
| 4   | Devices and the agent protocol come before activity and screenshots                              | Ingest, DPoP tokens and sync batches are prerequisites for all telemetry                                      |
| 5   | Desktop work runs in parallel from stage 1 (OS spikes), not after time tracking                  | The agent has the longest critical path and the most unknowns (Phase 1 risks)                                 |
| 6   | Billing moves ahead of integrations and mobile                                                   | Public Beta must be able to charge; integrations and mobile are V1                                            |

**Team assumed for this plan** (adjust sizes if different): 1 tech lead
(you), 2 Laravel engineers, 1 front-end engineer, 1 senior Rust
engineer + 1 desktop engineer, 1 QA/DevOps engineer, a part-time
designer; a mobile engineer joins at stage 7 and a data/ML engineer at
stage 9.

## 2–4. Epics, features and stories

36 epics hold 262 stories. Each epic's full card (purpose, dependencies,
features, tables, APIs, frontend, desktop, mobile, tests) and its story
table (description, acceptance criteria, dependencies, size, tables,
APIs, apps) are in the Epics & stories tab.

| ID  | Epic                                     | Depends on         | Stories | First release      |
|-----|------------------------------------------|--------------------|---------|--------------------|
| E00 | Engineering foundation                   | —                  | 9       | Alpha              |
| E01 | Platform kernel                          | E00                | 8       | Alpha              |
| E02 | Identity and authentication              | E01                | 10      | Alpha              |
| E03 | Tenancy and organizations                | E02                | 9       | Alpha              |
| E04 | Members, RBAC and teams                  | E03                | 8       | Alpha              |
| E05 | Policies, entitlements, privacy notice   | E04                | 5       | Alpha              |
| E06 | Projects, clients, tasks, rates, budgets | E04                | 7       | Alpha              |
| E07 | Devices and agent protocol               | E05                | 9       | Alpha              |
| E08 | Time tracking core                       | E06, E07           | 11      | Alpha              |
| E09 | Desktop agent core                       | E07, E08           | 14      | Alpha              |
| E10 | Desktop: macOS                           | E09                | 9       | Alpha              |
| E11 | Desktop: Windows                         | E09                | 8       | Alpha              |
| E12 | Desktop: Ubuntu X11                      | E09                | 6       | Private Beta       |
| E13 | Desktop: Ubuntu Wayland                  | E12, E35           | 6       | V1                 |
| E14 | Activity ingestion and rollups           | E07, E08           | 6       | Alpha              |
| E15 | Screenshots                              | E14, E05           | 7       | Alpha              |
| E16 | Applications and websites                | E14                | 5       | Beta               |
| E17 | Web app and dashboards                   | E04, E08, E14      | 7       | Alpha              |
| E18 | Reports and exports                      | E14, E16           | 6       | Alpha              |
| E19 | Timesheets and approvals                 | E08                | 5       | Private Beta       |
| E20 | Billing and subscriptions                | E05, E04           | 8       | Private Beta       |
| E21 | Notifications and alerts                 | E01, E04           | 4       | Beta               |
| E22 | Public API, webhooks, developer platform | E02, E18           | 7       | V1                 |
| E23 | Integrations                             | E01, E06, E08      | 11      | V1                 |
| E24 | Mobile app                               | E02, E07, E08, E19 | 8       | V1                 |
| E25 | Scheduling and attendance                | E08, E21           | 5       | V2                 |
| E26 | Breaks, time off, holidays, overtime     | E08, E19           | 6       | V2                 |
| E27 | Locations and job sites                  | E24                | 6       | V2                 |
| E28 | Expenses                                 | E06                | 4       | V2                 |
| E29 | Payroll and invoicing                    | E19, E06, E26      | 8       | V1 (invoices), V2  |
| E30 | Enterprise identity and administration   | E02, E04, E09      | 8       | V3                 |
| E31 | Privacy operations                       | E14, E15           | 6       | V2 / V3            |
| E32 | Insights analytics                       | E16, E18           | 7       | V3                 |
| E33 | AI                                       | E32, E18, E09      | 8       | V3                 |
| E34 | Scale and resilience                     | continuous         | 8       | Public Beta onward |
| E35 | Browser extension                        | E08, E09           | 3       | V1                 |

Size mix: 23 XL stories (security, OS-native code, money, sync) and 1
XXL (E34-S08) that must be split before it starts; the other 238 are S,
M or L.

## 5–6. Database migration order and API implementation order

**Migrations.** 150 schema migrations and 7 seed migrations, ordered so
no table is created before the tables it references, and grouped by
release (MVP 000–088, V1 089–107, V2 108–138, V3 139–149). Every tenant
table ships with its RLS policy in the same migration. Full list:
Migrations.

**API implementation order.** Each group is built only after the groups
it depends on, and each endpoint ships with its OpenAPI entry, policy
and isolation test.

| Order | Group                                                   | Endpoints                                                                                                                   | Epic               | Release      |
|-------|---------------------------------------------------------|-----------------------------------------------------------------------------------------------------------------------------|--------------------|--------------|
| 1     | Platform                                                | /health, /ready, /v1/jobs/{id}; error, pagination, idempotency behaviour                                                    | E00, E01           | Alpha        |
| 2     | Auth core                                               | /oauth/*, /.well-known/*, /v1/auth/register, login, refresh, logout, me, sessions, password/\*                              | E02                | Alpha        |
| 3     | Organizations                                           | /v1/organizations\*, /v1/auth/switch-organization                                                                           | E03                | Alpha        |
| 4     | Users and teams                                         | /v1/users\*, /v1/users/invitations\*, /v1/invitations/accept, /v1/roles (read), /v1/teams\*                                 | E04                | Alpha        |
| 5     | Policies and entitlements                               | /v1/organizations/current/policies\*, /v1/billing/entitlements                                                              | E05                | Alpha        |
| 6     | Projects and tasks                                      | /v1/clients\*, /v1/projects\*, /v1/tasks\*                                                                                  | E06                | Alpha        |
| 7     | Agent protocol                                          | /v1/desktop/register, config, projects, notice/acknowledge, sync, heartbeat, commands, capabilities, updates; /v1/devices\* | E07                | Alpha        |
| 8     | Time                                                    | /v1/timer\*, /v1/time-entries\*, /v1/idle-periods\*                                                                         | E08                | Alpha        |
| 9     | Activity                                                | /v1/activity/\*; sync slot.closed handler                                                                                   | E14                | Alpha        |
| 10    | Screenshots                                             | /v1/desktop/screenshots/upload-urls, /v1/screenshots\*                                                                      | E15                | Alpha        |
| 11    | Reports                                                 | /v1/reports/types, query, exports, dashboard                                                                                | E18                | Alpha        |
| 12    | Apps and websites                                       | /v1/applications\*, /v1/websites\*; sync usage handlers                                                                     | E16                | Beta         |
| 13    | Auth hardening                                          | /v1/auth/mfa/\*, step-up, email-code, sessions/revoke-all                                                                   | E02                | Beta         |
| 14    | Manual time and timesheets                              | /v1/manual-time-requests\*, /v1/timesheets (view, submit)                                                                   | E08, E19           | Private Beta |
| 15    | Notifications                                           | /v1/notifications\*, /v1/notifications/preferences                                                                          | E21                | Beta         |
| 16    | Billing                                                 | /v1/billing/\*, /hooks/billing/{provider}                                                                                   | E20                | Private Beta |
| 17    | Approvals, rates, budgets                               | timesheet approve/deny/reopen/bulk; /v1/rates\*, /v1/budgets\*                                                              | E19, E06           | V1           |
| 18    | Developer platform                                      | /v1/webhooks\*, third-party OAuth apps, PAT and org tokens, /v1/auth/passkeys\*                                             | E22, E02           | V1           |
| 19    | Integrations                                            | /v1/integrations\*, /hooks/integrations/{provider}                                                                          | E23                | V1           |
| 20    | Client invoices, saved reports, alerts, classifications | /v1/invoices\*, /v1/reports/saved\*, schedules\*, /v1/alert-rules\*, /v1/classifications\*                                  | E29, E18, E21, E16 | V1           |
| 21    | Workforce                                               | /v1/schedules/*, /v1/attendance*, break policies, /v1/timer/breaks/*, /v1/time-off/*                                        | E25, E26           | V2           |
| 22    | Locations                                               | /v1/attendance/job-sites\*, visits, locations, office-networks                                                              | E27                | V2           |
| 23    | Money                                                   | /v1/expenses\*, /v1/payroll/\*                                                                                              | E28, E29           | V2           |
| 24    | Enterprise                                              | /v1/auth/sso/*, /scim/v2/*, /v1/roles (write), /v1/audit-logs\*                                                             | E30                | V3           |
| 25    | Privacy operations                                      | /v1/privacy/requests\*, /v1/privacy/legal-holds\*                                                                           | E31                | V2/V3        |
| 26    | Insights and AI                                         | /v1/insights/*, /v1/ai/*, /internal/ai/\*                                                                                   | E32, E33           | V3           |

**Additions to the Phase 2 API catalog** found while planning (to be
added to the API tab): /v1/notifications\* and
/v1/notifications/preferences, /v1/alert-rules\*,
/v1/privacy/requests\*, /v1/privacy/legal-holds\*, /v1/insights/\*, and
the Reverb /broadcasting/auth endpoint.

## 7. Desktop implementation plan

One shared Rust core (E09) and four OS workstreams (E10–E13). macOS and
Windows ship first because most paying teams use them; X11 follows for
Private Beta; Wayland ships in V1 with reduced, clearly labelled
capability.

| Aspect       | macOS (E10)                                                                                                                                           | Windows (E11)                                                                                                                                             | Ubuntu X11 (E12)                                                                                                                   | Ubuntu Wayland (E13)                                                                                                                                         |
|--------------|-------------------------------------------------------------------------------------------------------------------------------------------------------|-----------------------------------------------------------------------------------------------------------------------------------------------------------|------------------------------------------------------------------------------------------------------------------------------------|--------------------------------------------------------------------------------------------------------------------------------------------------------------|
| Native APIs  | ScreenCaptureKit; CGEventTap; CGEventSourceSecondsSinceLastEventType; NSWorkspace; AppleScript / AX for URLs; SMAppService; IOKit power notifications | Windows.Graphics.Capture (+ DXGI Desktop Duplication fallback); Raw Input; GetLastInputInfo; GetForegroundWindow; UI Automation; power broadcast; Run key | XShm / XGetImage + XRandR; XInput2; XScreenSaver; EWMH \_NET_ACTIVE_WINDOW; AT-SPI; StatusNotifierItem; XDG autostart              | xdg-desktop-portal (Screenshot/ScreenCast); ext-idle-notify-v1 or Mutter IdleMonitor (D-Bus); GNOME Shell extension; KWin script; browser extension for URLs |
| Permissions  | Screen Recording, Accessibility, Input Monitoring, Automation per browser, Notifications; re-checked on wake                                          | None for standard user; SmartScreen reputation; optional admin for the update service                                                                     | None typical; AT-SPI must be enabled for URL fallback                                                                              | Portal consent (once where persistence is supported); GNOME extension install; no global input or window access                                              |
| Rust modules | platform::macos::{capture, input, idle, window, url, tcc, tray, autostart, power, network} via objc2 bindings                                         | platform::windows::{capture, input, idle, window, url, tray, autostart, power, network, service} via windows-rs                                           | platform::linux::x11::{capture, input, idle, window, url} + shared linux::{tray, autostart, power, network} via x11rb, atspi, zbus | platform::linux::wayland::{detect, portal_capture, idle, window_gnome, window_kde} + shared Linux modules via ashpd, zbus, wayland-client                    |
| React UI     | Permission wizard with System Settings deep links; revoked-permission banner                                                                          | "Start with Windows" toggle; SmartScreen help                                                                                                             | AT-SPI hint when URLs missing                                                                                                      | "Reduced on Wayland" panel listing what works; extension install prompt                                                                                      |
| Tests        | Matrix: macOS 13–15 and current, Apple Silicon + Intel; TCC deny/revoke scenarios; notarization check                                                 | Windows 10 22H2, 11; DPI 100–200%; standard user; AV false-positive scan                                                                                  | Ubuntu 22.04/24.04 X11, GNOME + KDE; Xvfb CI capture tests                                                                         | Ubuntu 24.04 GNOME Wayland, Fedora GNOME; portal flow with persisted permission; activity parity check vs X11                                                |
| Packaging    | Universal .dmg + .pkg (MDM)                                                                                                                           | .msi (WiX) + .exe (NSIS), per-user                                                                                                                        | .deb, .rpm, signed apt/dnf repositories                                                                                            | Same packages as X11 + GNOME extension on extensions.gnome.org                                                                                               |
| Signing      | Developer ID Application + Installer; hardened runtime; notarization and stapling in CI                                                               | Authenticode with EV certificate held in a cloud HSM; timestamping                                                                                        | GPG-signed repositories and packages                                                                                               | Same as X11; extension signed by the GNOME review process                                                                                                    |
| Auto-update  | Tauri updater (signed) with stable/beta channels, staged rollout, forced minimum version                                                              | Tauri updater + optional update service for silent installs                                                                                               | apt/dnf repository updates; in-app prompt                                                                                          | Same as X11                                                                                                                                                  |

**Sequence.** (1) Stage 1: one spike per OS (E10–E13 S01) to confirm
APIs and prompts, producing go/no-go notes. (2) Stage 3–4: shared core
first (E09-S01–S06) against fake adapters and the simulation harness;
macOS and Windows adapters in parallel. (3) Stage 5: screenshots and URL
adapters; signing and notarization pipelines. (4) Private Beta: X11. (5)
V1: Wayland GNOME and the browser extension bridge. (6) V3: MDM silent
packages. KDE Wayland is Future.

## 8. Mobile implementation plan

The mobile app starts at stage 7 for V1 (timer, timesheets, requests,
push), then adds GPS, job sites and receipts in V2. It reuses the agent
protocol, so no new sync server work is needed.

| Phase                       | Scope                                                                                                     | Stories      | Release |
|-----------------------------|-----------------------------------------------------------------------------------------------------------|--------------|---------|
| M1 — Foundation             | Expo dev build, PKCE sign-in with app links, secure storage, DPoP, org switch, notice acknowledgement     | E24-S01, S06 | V1      |
| M2 — Offline core           | SQLite outbox, shared event envelope, background sync windows                                             | E24-S02      | V1      |
| M3 — Time                   | Timer, switch, breaks (V2), project/task picker, today and week totals                                    | E24-S03      | V1      |
| M4 — Approvals and requests | Timesheets view/submit, manual-time requests, manager approvals, time-off requests (V2)                   | E24-S04      | V1      |
| M5 — Push and release       | APNs/FCM, deep links, EAS build/submit, TestFlight and Play internal track, staged rollout                | E24-S05, S07 | V1      |
| M6 — Location               | Background location module spike and choice, tracking modes, permission flows, battery budget             | E27-S01      | V2      |
| M7 — Job sites              | Geofence registration (nearest 20 on iOS, 100 on Android), arrival/departure actions, restricted tracking | E27-S03, S04 | V2      |
| M8 — Expenses               | Receipt capture, offline upload                                                                           | E28-S03      | V2      |
| M9 — Insights               | Own insight feed (read)                                                                                   | E33          | V3      |

**Mobile tests:** Jest unit tests for state and outbox; Maestro E2E
flows on iOS and Android simulators in CI; real-device farm
(BrowserStack or AWS Device Farm) for OEM battery killers; battery
budget tests (\< 2%/hour without GPS, \< 3%/hour in timer-only GPS
mode); offline tests (airplane mode for 24 h, then sync).

## 9. Testing plan

Every feature is tested at the levels below; the multi-tenant isolation
suite and the offline-sync property tests are release blockers, not
optional extras.

**Test types and tools**

| Type                   | What it proves                                                               | Tools                                                                                                      | Runs                                          |
|------------------------|------------------------------------------------------------------------------|------------------------------------------------------------------------------------------------------------|-----------------------------------------------|
| Unit                   | Domain rules in isolation (rates, overtime, accruals, slot maths)            | Pest (PHP), Vitest (TS), cargo test + proptest (Rust), Jest (mobile)                                       | Every PR                                      |
| Integration            | A domain service against real PostgreSQL, Redis, MinIO                       | Pest with Testcontainers                                                                                   | Every PR                                      |
| Feature                | A full HTTP request through middleware, policy and database                  | Pest feature tests                                                                                         | Every PR                                      |
| API / contract         | Responses match OpenAPI; generated client compiles; Spectral lint            | Schemathesis, Spectral                                                                                     | Every PR                                      |
| E2E (web)              | Owner, manager and member journeys in a browser                              | Playwright against an ephemeral environment                                                                | Merge to main; nightly                        |
| Desktop                | Core logic against fake adapters; real OS adapters on real machines          | Simulation harness (E09-S14); CI runners on macOS, Windows, Ubuntu; manual matrix before release           | Every PR (harness); release (matrix)          |
| Mobile                 | Flows on simulators and real devices; battery; offline                       | Maestro, device farm                                                                                       | Merge; release                                |
| Security               | Known vulnerability classes, secrets, dependencies, auth flows               | Semgrep, PHPStan, cargo audit, npm/Composer audit, ZAP DAST on staging, SAML test suite, annual pen test   | Every PR (static); weekly (DAST); pre-release |
| Load                   | Ingest, reports and dashboards at target scale                               | k6 with an agent simulator (10k → 100k agents)                                                             | Weekly on staging; before each release        |
| Multi-tenant isolation | No cross-tenant access through any route, query, cache, job, file or channel | Cross-tenant replay harness (E03-S07); RLS fail-closed suite; Octane alternating-tenant test; S3 key tests | Every PR — build fails on any leak            |

**Per-feature coverage**

| Feature area                     | Unit                            | Integration / feature                             | API        | E2E                         | Desktop / mobile                                 | Security                                 | Load                 | Isolation                |
|----------------------------------|---------------------------------|---------------------------------------------------|------------|-----------------------------|--------------------------------------------------|------------------------------------------|----------------------|--------------------------|
| Auth and sessions (E02)          | Token rotation, TOTP windows    | Login, refresh reuse, lockout                     | Contract   | Sign-up → MFA → sign-in     | Loopback PKCE, DPoP                              | Brute force, token theft, CSRF           | Login burst          | Tokens bound to org      |
| Tenancy (E03)                    | Context object                  | RLS, composite FKs                                | —          | Org switch                  | Org choice at sign-in                            | Pen test focus                           | —                    | Full harness             |
| RBAC (E04)                       | Resolver rules                  | Permission matrix per role                        | 403 vs 404 | Role-based menus            | —                                                | Privilege escalation                     | Resolver cache       | Role scope               |
| Policies and notice (E05)        | Resolution order                | Entitlement enforcement                           | Contract   | Change policy → agent obeys | Notice gate                                      | Tampered config                          | —                    | Per-org cache            |
| Projects and tasks (E06)         | Rate overlap, budget maths      | Archive stops timers                              | Contract   | Create project, assign      | Picker delta                                     | —                                        | —                    | Foreign IDs              |
| Agent protocol (E07)             | Verdict mapping                 | Batch replay identical                            | Contract   | —                           | Sync against staging                             | DPoP replay, device spoofing             | 10k agents           | Device ↔ member ↔ org    |
| Time tracking (E08)              | Overlap rules                   | Exclusion constraint, locks                       | Contract   | Web timer + edits           | Takeover between devices                         | Tampered timestamps                      | Timer bursts         | Foreign entries          |
| Desktop core and OS (E09–E13)    | Slot aggregation, state machine | —                                                 | —          | —                           | Simulation harness, crash injection, OS matrices | Local store encryption, update signature | 7-day offline replay | Wrong-org token rejected |
| Activity and rollups (E14)       | Rollup maths                    | Rollup = base (property)                          | Contract   | Activity page               | —                                                | —                                        | Ingest + rollup lag  | Rollup rows scoped       |
| Screenshots (E15)                | Retention dates                 | Presign constraints, processing                   | Contract   | View, delete                | Capture pipeline                                 | Presigned URL abuse, bucket policy       | Upload throughput    | S3 key and KMS context   |
| Apps and URLs (E16)              | Normalisation                   | Server-side privacy level                         | Contract   | Apps page                   | Adapter accuracy                                 | Path leakage                             | Usage ingest         | —                        |
| Reports (E18)                    | Query classes                   | Golden files incl. DST                            | Contract   | Build and export report     | —                                                | Export access                            | Large exports        | Scope per report         |
| Timesheets (E19)                 | State machine                   | Locking                                           | Contract   | Submit → approve            | Mobile approve                                   | Self-approval                            | —                    | Scope                    |
| Billing (E20)                    | Proration                       | Webhook idempotency                               | Contract   | Upgrade flow (sandbox)      | Suspension stops agent                           | Webhook signature                        | —                    | Seats per org            |
| Integrations (E23)               | Mapping                         | Recorded fixtures per provider                    | Contract   | Connect Jira                | Imported tasks in picker                         | OAuth state, SSRF                        | Sync fan-out         | Credentials per org      |
| Mobile and locations (E24, E27)  | Outbox                          | —                                                 | Contract   | —                           | Maestro, device farm, battery                    | Location privacy                         | —                    | —                        |
| Workforce (E25–E26)              | Accruals, overtime, recurrence  | Ledger idempotency                                | Contract   | Shift → attendance          | Break buttons                                    | —                                        | Evaluation job       | Scope                    |
| Money (E28–E29)                  | Money property tests            | Provider idempotency, numbering under concurrency | Contract   | Run payroll (sandbox)       | Payslip view                                     | Step-up, payout account changes          | Payroll batch        | Per-org payouts          |
| Enterprise and privacy (E30–E31) | Custom role limits              | SCIM validators, DSR completeness                 | Contract   | SSO sign-in                 | SSO on mobile                                    | SAML attacks                             | —                    | Staff access audited     |
| AI (E32–E33)                     | Metric definitions              | Number validator, golden sets                     | Contract   | Ask a question              | —                                                | Prompt injection, scope refusal          | Nightly batch        | Analytics row policies   |

## 10. CI/CD

Code moves through four environments; nothing reaches production that
has not passed the same checks on staging, and every deploy can be
rolled back without a database restore.

| Stage          | Trigger                             | Lint and static analysis                                                                                  | Tests                                                                                                | Migrations                                                                                         | Builds                                                                    | Desktop installers and signing                                                                                     | Release                                                                                             | Rollback                                                                                                                                                                                         |
|----------------|-------------------------------------|-----------------------------------------------------------------------------------------------------------|------------------------------------------------------------------------------------------------------|----------------------------------------------------------------------------------------------------|---------------------------------------------------------------------------|--------------------------------------------------------------------------------------------------------------------|-----------------------------------------------------------------------------------------------------|--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| Development    | Every push / PR                     | Pint, PHPStan (max), ESLint, tsc, rustfmt, Clippy, Spectral, Terraform fmt/validate, Semgrep, secret scan | Unit, integration, feature, API contract, isolation harness, desktop simulation, Rust property tests | Run on an ephemeral database from scratch; migration lint (RLS present, no locking DDL)            | API image, web bundle, unsigned desktop builds (3 OSes), mobile JS bundle | Unsigned artifacts only                                                                                            | Preview environment per PR (web + API)                                                              | Close PR                                                                                                                                                                                         |
| Testing (main) | Merge to main                       | Same                                                                                                      | Same + Playwright E2E + Maestro on simulators                                                        | Applied to the shared dev database                                                                 | Versioned images and artifacts                                            | Signed nightly desktop builds (beta keys) to the internal channel                                                  | Auto-deploy to dev                                                                                  | Revert commit                                                                                                                                                                                    |
| Staging        | Release branch or tag vX.Y.Z-rc     | Same                                                                                                      | Full E2E, DAST (ZAP), k6 load, OS test matrix, device farm                                           | Applied with production settings (expand phase only) on a production-sized copy of anonymised data | Release candidate images                                                  | Production-signed desktop builds (Developer ID + notarization, EV Authenticode, GPG) published to the beta channel | Deploy to staging; beta agents update                                                               | Redeploy previous image; agents pinned to previous version                                                                                                                                       |
| Production     | Manual approval of a staged release | —                                                                                                         | Smoke tests post-deploy; synthetic checks                                                            | Pre-deploy ECS task runs expand migrations; contract migrations only in a later release            | Promote the exact staging images (no rebuild)                             | Same signed installers promoted to stable with staged rollout (5% → 25% → 100%)                                    | Rolling or blue/green ECS deploy; feature flags for risky features; mobile via store staged rollout | Previous task definition in one click; feature-flag kill switches; desktop: halt rollout and publish previous version as newer build; mobile: halt staged rollout, EAS Update revert for JS-only |

**Guardrails.** Main is protected (review + green checks). Database
changes follow expand/contract so the previous app version always runs
on the new schema. Signing keys live in HSM-backed services (Apple via
App Store Connect API keys, Windows EV certificate in a cloud HSM, GPG
in KMS-backed signer); CI never stores raw private keys. Every
production deploy records the version in the status page and audit log.

## 11. Release plan

Five releases widen the audience step by step. Each has an exit
criterion that must hold before the next begins.

| Release                   | Audience                                        | Features included (new in this release)                                                                                                                                                                                                                                                                                                                                 | Exit criteria                                                                                   |
|---------------------------|-------------------------------------------------|-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|-------------------------------------------------------------------------------------------------|
| **Alpha**                 | Worklume team + 1–2 friendly orgs (≤ 20 people) | Core of E00–E11, E14, E15, E17, E18: sign-up, orgs, members, roles, teams, policies, privacy notice; projects, clients, tasks; macOS and Windows agents with timer, activity %, idle, offline sync, screenshots (blur, notification, delete, retention); web timer; time views and manual time; activity and screenshot pages; dashboard; core reports with CSV/PDF     | 2 weeks of daily use with zero lost slots; isolation harness green; crash-free sessions ≥ 99%   |
| **Beta**                  | 3–5 design-partner orgs (≤ 150 people), free    | App and URL tracking (domain-only default); TOTP MFA and step-up; email notifications; onboarding checklist; manual-time approvals; agent auto-update channels                                                                                                                                                                                                          | Partners confirm data trust; agent CPU \< 3% average; no P1 bugs open                           |
| **Private Beta**          | Waitlist, 20–50 orgs, invite only               | Ubuntu X11 agent; timesheets (view, submit, lock); billing in provider test mode; entitlement gating; plan limits; weekly digests                                                                                                                                                                                                                                       | Activation (first tracked hour) ≥ 60% of invited orgs; billing flows pass end to end in sandbox |
| **Public Beta** (= MVP)   | Self-serve sign-up, paid (beta discount)        | Live billing, trials, dunning; marketing site; security hardening; external pen test (tenant isolation focus); load test at 10,000 simulated agents; status page                                                                                                                                                                                                        | 30 days at SLO; first paying customers retained; pen-test findings fixed                        |
| **Production** (= V1, GA) | General availability                            | Timesheet approvals and locking; rates, budgets, client invoices; limits; mobile apps (timer, timesheets, requests, push); public API, OAuth apps, webhooks, Zapier; Jira, Asana, ClickUp, GitHub, Slack integrations; saved and scheduled reports; alert rules; productivity classification; passkeys; PATs; Wayland (GNOME, reduced); browser extension; SOC 2 Type I | SOC 2 Type I report; 99.9% availability for 60 days; support runbooks                           |

V2 and V3 then ship as feature releases on the production train (monthly
web/API releases; desktop and mobile every 2–4 weeks).

## 12. MVP

The smallest Worklume a team would pay for: accurate, trustworthy work
tracking with visibility for managers — on macOS, Windows and Ubuntu —
built privacy-first. It is everything up to Public Beta.

| Area         | In the MVP                                                                                                                   | Deliberately later                                          |
|--------------|------------------------------------------------------------------------------------------------------------------------------|-------------------------------------------------------------|
| Organization | Sign-up, orgs, members, invitations, 8 built-in roles, teams and team leads, policy hierarchy                                | Custom roles, SSO, SCIM (V3)                                |
| Tracking     | Desktop agent on macOS, Windows, Ubuntu X11; timer; activity %; idle detection and resolution; offline-first sync; web timer | Wayland, browser extension, mobile (V1)                     |
| Evidence     | Screenshots (frequency, blur, notification, deletion, retention, access log); app and URL tracking at domain level           | Full URL paths and window titles (opt-in V1); Insights (V3) |
| Time         | Manual time with reasons, approvals of manual time, time views, timesheets (view, submit, lock)                              | Timesheet approval chains, limits (V1)                      |
| Projects     | Projects, clients, tasks                                                                                                     | Rates, budgets, invoices (V1)                               |
| Reporting    | Dashboard, Time & Activity, Daily Totals, Work Sessions, Manual Edits, Apps & URLs; CSV/PDF export                           | Saved and scheduled reports (V1)                            |
| Commercial   | Plans, seats, trial, checkout, dunning, suspension, entitlements                                                             | Add-ons beyond the base plans (V1+)                         |
| Security     | TOTP MFA, step-up, audit log (internal), isolation harness, pen test                                                         | Passkeys, API tokens (V1)                                   |

**Differentiation kept in the MVP:** privacy-first defaults and a
monitoring notice employees must acknowledge; a "my data" view where
every employee sees their own screenshots and activity; honest
per-device capability reporting (no silent gaps); offline-first sync
that never loses time; Linux support from launch; a screenshot access
log.

**One change from Phase 1:** app and URL tracking moves from V1 into the
MVP (domain-only by default). Without it the productivity view is too
thin to compete with Hubstaff, and the domain-only default keeps the
privacy stance.

## 13. V1 — first production-grade release

V1 makes Worklume a complete team product and a platform others can
build on: timesheet approvals with locking, limits, rates and budgets,
client invoices (recurring, reminders, aging); iOS and Android apps
(timer, timesheets, manual-time and approval flows, push); public REST
API with OAuth apps, PATs and org tokens, webhooks (including time-entry
update/delete events), Zapier; Jira, Asana, ClickUp, GitHub, GitLab and
Slack/Teams integrations; saved and scheduled reports; alert rules (low
activity, idle, device offline, budget); productivity classification by
org and team; passkeys; Wayland on GNOME (reduced, labelled) and the
browser extension; SOC 2 Type I.

## 14. V2 — advanced workforce

V2 covers the whole working day and the pay cycle: schedules and
recurring shifts with late/missed/abandoned tracking and attendance;
automatic start by shift; breaks; time off with accrual ledger,
balances, requests and holidays; overtime (weekly, daily, double time);
GPS tracking modes, geofenced job sites with arrival/departure actions,
office-vs-remote detection; expenses with receipts and approvals;
payroll runs, payouts (Wise, PayPal, Deel, Gusto, Payoneer, Remote),
payslips, team invoices, external payroll export; QuickBooks, Xero,
BambooHR and calendar integrations; retention settings, legal holds and
aggregate-only mode.

## 15. V3 — AI, analytics, integrations and enterprise

V3 targets larger customers and intelligence: SAML/OIDC SSO, SCIM,
custom roles, audit log viewer/export/SIEM streaming, silent MDM agents,
IT admin tools, enterprise billing, break-glass tooling;
data-subject-request tooling and a second regional cell (data
residency); the Insights add-on (analytics store, focus, utilization,
meetings, job-type templates) and ClickHouse at Stage C; AI (anomaly
review, automated-input detection, daily/weekly summaries,
natural-language reporting, workload analysis, recommendations,
classification suggestions) with evaluation and shadow mode; MCP server.
Future: KDE Wayland, benchmarks, kanban tasks, Chromebook, marketplace,
field-service work orders.

## 16. Master backlog

The backlog is organised epic → feature → story → task and sized XS–XXL.
It lives in the Master backlog tab: 36 epics, 205 features, 262 stories.
Every story expands into the standard task set defined there (migration,
domain logic, policy, API and OpenAPI, UI, client work, tests,
observability, docs); only story-specific extra tasks are listed
individually.

## 17. Definition of Done

A story or feature is done only when every applicable line below is
true. The reviewer checks the list in the pull request template; "not
applicable" must be stated, not assumed.

| \#  | Criterion                                                                                                        | How it is checked                                                                       |
|-----|------------------------------------------------------------------------------------------------------------------|-----------------------------------------------------------------------------------------|
| 1   | Backend implemented in the owning domain module, with no cross-module internals                                  | Architecture tests green                                                                |
| 2   | Database migration complete: table, indexes, composite FKs, RLS policy, grants; expand/contract safe             | Migration lint; migrates from scratch and from the previous release                     |
| 3   | Authorization complete: policy on every action; permissions added to the catalog; sensitive permissions explicit | Route-policy CI check; permission matrix tests                                          |
| 4   | Tenant isolation tested                                                                                          | Cross-tenant replay harness covers the new routes, jobs, cache keys, files and channels |
| 5   | API documented: OpenAPI entry with examples, error codes and idempotency                                         | Spectral lint; contract tests; docs build                                               |
| 6   | Frontend implemented, accessible (WCAG 2.2 AA) and localisable                                                   | Playwright journey; axe check                                                           |
| 7   | Required desktop and mobile functionality implemented                                                            | Simulation harness and OS matrix (desktop); Maestro flow (mobile)                       |
| 8   | Tests written: unit, integration/feature, API; E2E for user-facing flows                                         | Coverage of new domain logic ≥ 90%; CI green                                            |
| 9   | Error handling complete: problem+json codes, retries, user-facing messages, idempotency where side effects exist | Review checklist; negative tests                                                        |
| 10  | Logging complete: structured logs with request_id and org_id, no personal data or secrets                        | Log review in staging                                                                   |
| 11  | Monitoring complete: metrics, traces, alerts and a dashboard panel for new jobs or endpoints                     | Alert fires in a staging test                                                           |
| 12  | Audit events written for security-relevant and admin actions                                                     | Audit log assertion in tests                                                            |
| 13  | Security reviewed: threat notes for auth, money, files, external calls; static and dependency scans clean        | Reviewer sign-off; Semgrep and audit jobs green                                         |
| 14  | Privacy reviewed: data class, retention, notice impact, employee visibility                                      | Privacy checklist in PR                                                                 |
| 15  | Performance within budget (p95 targets from Architecture §21)                                                    | Load test or benchmark for hot paths                                                    |
| 16  | Feature flag and rollback plan for risky changes                                                                 | Flag present; rollback step written                                                     |
| 17  | Documentation updated: user help, admin docs, API changelog, runbook if operational                              | Docs PR linked                                                                          |
| 18  | Deployed to staging and verified by someone other than the author                                                | QA sign-off in the ticket                                                               |

## 18. Critical path

The MVP date is set by one chain: tenancy → agent protocol → time model
→ desktop core → OS adapters → telemetry → billing. Any slip there moves
Public Beta; work elsewhere has slack.

<img src="media/image2.png" style="width:6in;height:2.125in"
alt="Critical path to MVP · 13 steps" />

Critical path to MVP · 13 steps

The riskiest links are E03-S04 (RLS), E07-S03 (sync batch framework),
E08-S02 (timer control), E09-S04/S05 (tracker and sync engine) and
E09-S07 (screenshot pipeline) — six XL stories. They get the most senior
engineers, start as early as their dependencies allow, and are protected
from interruptions. The OS spikes (E10–E13 S01) run in stage 1 so the
desktop core is never waiting on unknowns.

## 19. Parallel workstreams

Seven workstreams run in parallel against the same stage plan; the table
shows what each works on for every release.

| Workstream                                      | Alpha                                                           | Beta                                        | Private Beta                 | Public Beta                                | V1                                                       | V2                                        | V3                              |
|-------------------------------------------------|-----------------------------------------------------------------|---------------------------------------------|------------------------------|--------------------------------------------|----------------------------------------------------------|-------------------------------------------|---------------------------------|
| Platform / backend (2 Laravel engineers + lead) | E01–E08, E14, E15, E18                                          | E16 ingest, E21 core, manual-time approvals | E19 submit/lock, E20 billing | E20 live, hardening, pen-test fixes        | E19 approvals, E22, E23, E29 invoices, E06 rates/budgets | E25, E26, E28, E29 payroll, E31 retention | E30, E31 DSR, E32               |
| Web (1 front-end engineer)                      | E00-S08, E17-S01–S04, time and screenshot views, report builder | Onboarding, apps/URLs pages                 | Timesheets, billing UI       | Marketing site, accessibility              | Approvals, developer settings, integrations UI           | Schedules, time off, map, payroll UI      | SSO wizard, Insights, AI panels |
| Desktop core (senior Rust engineer)             | E09 core, sync, screenshots, updater                            | Usage aggregation, UX polish                | Stability, soak tests        | Release hardening                          | Extension bridge (E35)                                   | Office network identity                   | Input features (E09-S13)        |
| Desktop OS (desktop engineer)                   | Spikes (all 4 OSes), macOS, Windows                             | macOS/Windows URLs                          | Ubuntu X11                   | Signing and store readiness                | Wayland GNOME                                            | —                                         | MDM packages                    |
| Mobile (joins at V1)                            | —                                                               | —                                           | —                            | Scaffold only                              | E24                                                      | E27, E28 receipts                         | Insight feed                    |
| QA / DevOps                                     | E00 infra, CI, isolation harness                                | Test matrices, dashboards                   | Load tests (k6)              | 10k-agent load test, pen test, status page | SOC 2 Type I evidence, DR drill                          | Device farm, cost dashboards              | Second cell, ClickHouse         |
| Data / AI (joins at V3)                         | —                                                               | —                                           | —                            | —                                          | —                                                        | Analytics store prep                      | E32, E33                        |

## 20. Major technical risks for delivery

These are the risks to executing this plan; the architecture risks
(Phase 2 §25) and product/OS risks (Phase 1 §15) still apply. The top
three threaten the MVP date directly.

| \#  | Risk                                                | Effect on the plan                                                                         | Early warning                                          | Mitigation                                                                                                     |
|-----|-----------------------------------------------------|--------------------------------------------------------------------------------------------|--------------------------------------------------------|----------------------------------------------------------------------------------------------------------------|
| D1  | No senior Rust engineer by stage 3                  | The whole critical path stalls at E09                                                      | Spikes (E10–E13 S01) slip past stage 1                 | Start hiring now; contract a Tauri specialist for the spikes; keep the agent UI thin so web engineers can help |
| D2  | Offline sync or conflict rules wrong                | Lost or duplicated time erodes trust in Alpha; rework across E07, E08, E09                 | Property tests fail or rollups disagree with base data | Write E08-S10 spec before E07-S03 code; property tests and 7-day soak test as Alpha exit gates                 |
| D3  | Tenant isolation gap found late                     | Launch blocked, re-architecture risk                                                       | Isolation harness coverage \< 100% of routes           | Harness built in stage 2 (E03-S07) and required on every PR; pen test before Public Beta                       |
| D4  | macOS permission friction                           | Low activation; support load                                                               | Onboarding drop-off at the permission wizard           | Wizard with deep links, help articles, permission health checks from Alpha                                     |
| D5  | Code-signing and store delays                       | Installers blocked by Apple notarization, Windows EV certificate or SmartScreen reputation | Certificates not issued by stage 3                     | Buy EV certificate and set up Apple developer account in stage 1; build reputation with beta users             |
| D6  | Billing provider not available for the legal entity | No revenue at Public Beta                                                                  | ADR-007 not accepted by stage 5                        | Decide in stage 1 (E20-S01 moved early if needed); keep the provider interface                                 |
| D7  | Scope creep from design partners                    | Beta drifts toward V2 features                                                             | Stories added to Beta without removing others          | Change control: any addition names what it displaces; V2 backlog is the default home                           |
| D8  | Wayland harder than planned                         | V1 slips if Wayland is treated as blocking                                                 | Portal persistence unsupported on target GNOME         | Wayland is not a V1 gate: ship "reduced" mode with clear labelling; X11 fallback documented                    |
| D9  | Integration provider approvals                      | Slack, Zapier, Microsoft and Atlassian app reviews take weeks                              | Submissions not made by early V1                       | Submit apps as soon as the minimal version works; launch with whichever are approved                           |
| D10 | Reporting correctness                               | Customers dispute payroll or invoices                                                      | Reconciliation mismatches                              | Money from base tables only; nightly reconciliation; finance review of test cases                              |
| D11 | Team capacity for parallel streams                  | Workstreams block each other                                                               | Critical-path stories waiting on reviewers             | Ownership by domain; review SLAs; lead keeps the critical path unblocked first                                 |
| D12 | Compliance timeline (SOC 2)                         | Enterprise deals slip                                                                      | Evidence collection not started by Public Beta         | Automate evidence (Vanta/Drata-style tooling) from Public Beta; SOC 2 Type I at V1                             |

**What to do this week:** (1) accept or amend the ADRs from Phase 2; (2)
start the Rust hiring and the Apple and Windows signing accounts; (3)
confirm the billing provider for your legal entity; (4) begin E00 and
the four OS spikes in parallel.
