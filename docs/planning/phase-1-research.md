# Worklume — Phase 1 Research: Hubstaff Capability Analysis & Requirements

Sep 28, 2026 · @Suni Kumar

## 1. Executive summary

Hubstaff is a proof-of-work time tracker with payroll, scheduling and analytics built around it. Worklume can match its functional scope, but the hard part is the desktop agent, not the web app.

**What Hubstaff is today (Sept 2026)**

- **Core loop.** A desktop timer records time in 10-minute slots. Each slot carries activity % (active seconds ÷ 600), up to 3 screenshots, and app/URL usage. Everything else is built on top of that slot: timesheets, approvals, payroll, invoices, budgets and reports.
- **Workforce management.** Schedules and shifts, breaks, time off with accruals, holidays, overtime, and job-site geofencing on mobile.
- **Analytics.** A paid "Insights" add-on ($2.50/seat) adds focus time, work-time classification, utilization, benchmarks and unusual-activity detection.
- **AI.** There is no built-in generative AI. The "AI" consists of rule-based and ML-assisted anomaly detection, plus an API, CLI and MCP server (Sept 2026) so outside assistants can query the data.
- **Platforms.** Windows, macOS and Linux desktop; iOS and Android; a web timer and Chrome extension. Linux support is thin: a self-extracting .sh installer, and on Wayland only GNOME screenshots are documented.
- **Price.** Roughly $5–12 per seat per month across Starter, Grow and Team. Enterprise is custom. Several features are sold as add-ons.

**What this means for Worklume**

1. **The desktop agent is the product.** Screenshot capture, activity sampling, app/URL detection, offline queueing and OS permissions decide trust and data quality. Plan it as its own engineering track from day one.
2. **Design the 10-minute activity slot as the core data unit.** Timesheets, reports, payroll and analytics all roll up from it.
3. **Wayland is a real gap to plan around, not to promise.** Hubstaff only partly solves it. Worklume should ship Wayland with clearly reduced capability behind portals and GNOME-specific paths.
4. **Differentiate on privacy and AI.** Hubstaff has no native AI summaries or recommendations, and its employee-facing transparency is modest. Both are openings.
5. **Storage and retention need planning.** Screenshots dominate storage cost. Hubstaff limits them per plan: 2 weeks to 6 months of screenshots, and a monthly screenshot cap on lower tiers.

**How to read this document**

| Tag | Meaning |
| --- | --- |
| **\[D\]** Documented | Stated on an official Hubstaff page that was opened. See Sources. |
| **\[I\]** Inferred | Implied by official pages or marketing, but not stated in the help center. |
| **\[U\]** Unknown | Not found in official sources. |
| **\[V\]** Requires validation | Official sources conflict, or the claim needs a hands-on test. |

All research used public Hubstaff sources as of 28 Sept 2026. Nothing here reflects Hubstaff's internal architecture or code; Worklume's design is our own.

## 2. Current Hubstaff feature inventory

Hubstaff covers ten functional domains. Plan tiers are shown where documented. The plans are Free, Small Business Essentials, Starter, Grow, Team and Enterprise.

### 2.1 Organization, roles and policies

| Feature | What Hubstaff does | Plan | Tag |
| --- | --- | --- | --- |
| Organizations | A user can own several organizations. Each has its own subscription and billing profile. Organizations can't be moved to another account. | All | \[D\] |
| Multi-org membership | One person belonging to and switching between several organizations is very likely, but not stated. | — | \[I\] |
| Org roles | Owner (billing, payroll, delete), Organization Manager (everything except billing and payroll), User (tracks only on assigned projects). | All | \[D\] |
| Project roles | Project Manager, Project User, and Project Viewer (read-only, free seat). | All | \[D\] |
| Team Lead | Team-level role. Each permission is switched on separately: approve timesheets, manual time and time off; create schedules; view activity; edit team projects; team finances. Cannot approve own timesheet. | Team+ | \[D\] |
| Extra permissions | "Manage IT" (SSO/SCIM, tracking policies, Silent app). An "IT Manager" role was announced Q2 2026. | Enterprise | \[D\] |
| Custom roles | Listed under Settings > Permissions, but no article describes them. | ? | \[U\] |
| Teams | Groups of members with leads, used for approvals, visibility and schedules. SCIM groups sync to teams. | Team+ | \[D\] |
| Policy hierarchy | An org-wide default, overridable per member. Covers idle timeout, screenshots, blur, deletion, breaks, Modify time, limits and tracking apps. | All | \[D\] |
| Settings groups | Organization, Members, Schedules, Activity & Tracking, Insights, Policies (time off, breaks, overtime), Integrations, Billing. | All | \[D\] |
| Invitations | Email invites, bulk CSV/XLS import (role, teams, projects, pay rate, pay period), and pending-invite editing. Expiry rules not stated. | All | \[D\] |
| Member removal | Time stays in reports. Screenshots and app/URL data are archived and come back if the member is re-invited. There is no "archive member" state. | All | \[D\] |

### 2.2 Time tracking

| Feature | What Hubstaff does | Tag |
| --- | --- | --- |
| Timer | Start/stop against a project and optional to-do on desktop, mobile, web and the Chrome extension. | \[D\] |
| Manual time | Project, start, end and a required reason. Can be edited, split and deleted. Counts as 0% activity and overwrites overlapping entries. | \[D\] |
| Manual time approvals | Pending, Approved or Denied, one at a time or in bulk. Approvers are owners, managers and team leads. | \[D\] |
| Timesheets | Daily, weekly and calendar views. The Modify time setting controls who can edit. | \[D\] |
| Timesheet approvals | Open → Submitted → Approved or Denied → Paid. Submitting locks the period. Batch approvals were added Q4 2025. Team plan or higher. | \[D\] |
| Locked time | Time is locked by a payment, invoice, approved timesheet, QuickBooks export or integration sync. Unlock rights depend on role. | \[D\] |
| Billable flag | Set on the project or on a manual entry. Budgets and invoices can include or exclude non-billable time. | \[D\] |
| Time zones | Views switch between member, viewer and org timezone. Reports, payments and dashboards always use the org timezone. | \[D\] |
| Limits | Daily and weekly caps, blocked days, shift-only tracking. Team plan or higher. | \[D\] |
| Idle handling | Timeout triggers keep, reassign, discard or stop. "Keep idle time" can be Prompt, Always or Never. Default timeout conflicts: 5 min in one article, 20 min in another. | \[D\] / \[V\] |
| Automatic start | Timer starts and stops on a schedule or on shifts, optionally waiting for first input. Desktop 1.6.14+, Team plan or higher. | \[D\] |
| Offline tracking | Stored locally and uploaded when the connection returns. | \[D\] |
| Rounding | No rounding setting documented. Time is recorded to the minute. | \[U\] |
| Timer on several devices at once | Not documented. "Desktop-only tracking" can block mobile. | \[U\] |

### 2.3 Projects, tasks and clients

| Feature | What Hubstaff does | Plan | Tag |
| --- | --- | --- | --- |
| Projects | Members by role or team, billable flag, client, budget. Can disable activity or idle tracking. Archive, restore, delete, and transfer between organizations. | All | \[D\] |
| To-dos | Simple to-dos in the desktop app and web timer. Capped at 500 (Starter), 1,500 (Grow), unlimited (Team). Hidden when a project is linked to an integration. | Varies | \[D\] |
| Hubstaff Tasks | A separate kanban product with lists, epics, estimates and checklists, and its own roles. | Grow+ or $2.50 add-on | \[D\] |
| Clients | Contact details, payment terms, recurring invoices, archive, and CSV import. Limits: 3 (Free and Essentials), 5 (Starter), unlimited (Grow and up). | All | \[D\] |
| Project budgets | Cost or hours, calculated on pay or bill rate. Resets weekly, monthly or never. Alert at a chosen %, optional auto-stop of timers. | Grow+ | \[D\] |
| Client budgets | Cost or hours. Resets never or monthly. Alert at a chosen %. | Team+ | \[D\] |
| Rates | Pay rate and bill rate per member, hourly or fixed, with effective dates. Per-project rates are not documented. | All | \[D\] |

### 2.4 Attendance and workforce management

| Feature | What Hubstaff does | Plan | Tag |
| --- | --- | --- | --- |
| Schedules and shifts | One-off or repeating shifts (weekly, bi-weekly) with minimum hours. Shifts can be created up to 30 days in the past. Month, week and day views. | Team+ | \[D\] |
| Shift status | On time, Late, Missed, Abandoned. Grace period 5–30 min. Alerts sent. | Team+ | \[D\] |
| Work breaks | Paid or unpaid. Policies set duration and when breaks are allowed. Break buttons in every app; alert on an abandoned break. | Grow+ | \[D\] |
| Time off | Paid or unpaid policies. Accrual annual, monthly, per hours worked, or by join date. Max balance, rollover, negative balances. Request and approval flow. | Team+ | \[D\] |
| Holidays | Preset or custom lists, assigned by member, country or employment type. | Team+ | \[D\] |
| Overtime | Weekly threshold × pay multiplier, by team or individual. Daily thresholds and double time not documented. | Team+ | \[D\] / \[U\] |
| Schedule adherence | Only as shift statuses and Insights utilization. No dedicated adherence score. | — | \[U\] |
| Job sites (geofences) | Mobile auto clock-in and clock-out, 50 m minimum radius. Timer can be restricted to sites. | Locations add-on | \[D\] |
| Remote vs office | Office detected by public IP and router MAC, auto-registered when 3+ members share a network in one day. | Insights | \[D\] |

### 2.5 Productivity

| Feature | What Hubstaff does | Tag |
| --- | --- | --- |
| Activity % | Each second is active if any keyboard or mouse input occurs. % = active seconds ÷ 600 per 10-min slot. Mobile, web and manual time are excluded. | \[D\] |
| App and URL tracking | Off, Apps only, or Apps & URLs, set per member. Desktop only. Capped at 500 (Starter) or 1,500 (Grow) per month; unlimited on Team. | \[D\] |
| Classification | Core, non-core, unproductive, unclassified, set per job type. How defaults are produced is not documented. | \[D\] / \[I\] |
| Insights widgets | Utilization, focus, meetings, work time classification, leaderboard, engagement score, benchmarks, highlights, timeline, unusual activity. | \[D\] |
| Dashboard | Customizable widgets: who is online, time by project, low activity. | \[D\] |

### 2.6 Screenshots

| Feature | What Hubstaff does | Tag |
| --- | --- | --- |
| Frequency | Off, 1, 2 or 3 per 10 min at random times. More with an add-on. Not taken during idle time. | \[D\] |
| Multiple monitors | Every monitor is captured on each shot. | \[D\] |
| Blur | Done on the device before upload and cannot be undone. Org-wide or per member. Paid plans only. | \[D\] |
| Notification | Optional toast when a shot is taken. On by default since v1.5.15. | \[D\] |
| Delete | Managers delete a screenshot and keep the time. Users delete the whole block, time included. Controlled by a setting. Permanent. | \[D\] |
| Retention | 2 weeks (Free), 1 month (Starter), 3 months (Grow), 6 months (Team and Enterprise). | \[D\] |
| Monthly cap | 100 (Free), 500 (Starter), 1,500 (Grow), unlimited (Team). | \[D\] |

### 2.7 Payroll, invoicing and expenses

| Feature | What Hubstaff does | Plan | Tag |
| --- | --- | --- | --- |
| Payroll | Hourly or fixed pay. Pay periods weekly, bi-weekly, semi-monthly or monthly. Automatic payroll after an approval window, or manual. Bonuses and one-time amounts. Mark as paid, or export. | Team+ ("limited" below) | \[D\] |
| Payout providers | Wise, PayPal, Payoneer, Bitwage, Deel, Gusto, Remote. Hubstaff holds no funds. | Team+ | \[D\] |
| Payslips | Payslips and proof of payment, Q4 2025. | ? | \[D\] |
| Client invoices | Built from tracked time × bill rate, plus expenses. Drafts, recurring invoices, reminders, PayPal payment link. Aging report. | All | \[D\] |
| Team invoices | Contractors invoice the organization. | Team+ | \[D\] |
| Expenses | Description, date, amount, category, project, receipt, billable flag. Can go on client invoices; mark as paid; Expenses report. | Grow+ (one source says Team+) | \[D\] / \[V\] |
| Expense approvals and categories | No approval workflow or category management documented. | — | \[U\] |

## 3. Desktop feature inventory

Only the desktop apps collect proof of work: screenshots, activity % and app/URL usage. The web timer, the mobile apps and the Chrome extension record time without them.

| Capability | Hubstaff desktop behaviour | Tag |
| --- | --- | --- |
| Supported OS | Windows 8+, macOS 10.12+, "Ubuntu 10.04 LTS or newer". No other distros named. Non-Apple ARM (Snapdragon) not supported. | \[D\] |
| Installers | Windows .exe (silent `/S`), MSI for Enterprise MDM. Linux self-extracting .sh (silent `-- --silent`). No .deb or .rpm documented. | \[D\] / \[U\] |
| Timer | Start/stop against project and to-do, compact always-on-top mode. | \[D\] |
| Screenshots | 0–3 per 10 min at random times, every monitor, blur on device, optional notification. | \[D\] |
| App tracking | Active app name and time. Off by default; turned on per member. | \[D\] |
| URL tracking | Chrome, Firefox, Edge, Safari, Island. Opera and Chrome Canary not supported. The desktop app reads URLs itself; no extension needed. | \[D\] / \[I\] |
| Keyboard/mouse activity | Active or inactive per second; keystrokes are not logged. Linux reports one combined keyboard+mouse figure. | \[D\] |
| Idle detection | Timeout 5/10/20 min, custom or never. Dialog: keep, reassign, discard or stop. Grow plan or higher. | \[D\] |
| Offline | Time, activity, screenshots, app/URL data cached with no time limit, uploaded on reconnect. | \[D\] |
| Sync | About every 10 minutes, plus a manual refresh button. | \[D\] |
| Tray / menu bar | Windows tray or taskbar. macOS menu bar, Dock or both. Linux tray icon. | \[D\] |
| Auto-start at login | Windows preference documented. macOS and Linux not documented. | \[D\] / \[U\] |
| Automatic timer start | Start and stop on schedule or shifts, optionally waiting for first input. | \[D\] |
| Updates | Manual check plus an opt-in auto-install on all three OSes. Disabled when managed by IT. macOS unattended updates not permitted. | \[D\] / \[V\] |
| Silent app (background) | Hidden, IT-deployed agent via MDM (Intune, Jamf, JumpCloud). Can track "all the time". $2.50 add-on, included with Enterprise. | \[D\] |
| Network identity | Collects public IP and router MAC for office detection. | \[D\] |
| macOS permissions | Screen Recording for screenshots. Automation (per browser) for URLs. Accessibility and Input Monitoring are not documented, but almost certainly required for input sampling. | \[D\] / \[I\] |

&#91;embedded content: Screenshot pipeline · desktop to dashboard, 6 steps\]

The blur step (3) is the key privacy decision: once applied on the device, the original image never leaves it. Steps 1–4 are the desktop agent's job; Worklume must build all of them itself.

## 4. Mobile feature inventory

The mobile apps are for time, location and attendance. They do not collect proof of work: no screenshots, no activity % and no app tracking.

| Capability | iOS | Android | Detail | Tag |
| --- | --- | --- | --- | --- |
| Minimum OS | iOS 16+ (help center) or 17+ (App Store) | Android 5 (SDK 21)+ | The two iOS sources conflict. | \[D\] / \[V\] |
| Timer, projects, to-dos | Yes | Yes | Can create or import tasks. | \[D\] |
| GPS location | Yes | Yes | Locations add-on ($3.33/seat), Team plan or higher; included with Enterprise. | \[D\] |
| Tracking modes | Yes | Yes | Off, while the timer runs, during shifts, or always (background, no timer). | \[D\] |
| Location cadence | On significant change or about every 150 m | About every 5 min | Points closer than 500 m are dropped unless nothing was recorded in 5 min. | \[D\] |
| Permissions | "Always" + Precise Location + Background App Refresh | High-accuracy location + background refresh |  | \[D\] |
| Live map | Yes | Not stated | Live map viewing is described as iOS only. | \[D\] / \[V\] |
| Job sites (geofences) | Yes | Yes | Minimum radius 50 m. Arrival: start timer, remind, or nothing. Departure: stop, notify, or nothing. Can restrict tracking to sites. | \[D\] |
| Attendance / shifts | Yes | Yes | Shift status, arrival and departure notifications. | \[D\] |
| Breaks | Yes | Yes | Break button in the timer. | \[D\] |
| Offline | Yes | Yes | Time and GPS cached and synced later. | \[D\] |
| Notifications | Yes | Yes | Push for alerts and reminders. Timer reminders are triggered by job sites only. | \[D\] |
| Screenshots / activity / app usage | No | No | Mobile time is excluded from activity %. | \[D\] |
| Idle detection | No | No |  | \[D\] |
| Battery cost | \~2%/hour | \~2%/hour | Marketing claim for the field product. | \[I\] |
| Field features | Work orders, route optimization, fleet tracking | Same | Marketed on the field-service page; no help-center detail. | \[I\] |

An admin setting, "Desktop-only time tracking", can stop members from tracking on mobile.

## 5. Web feature inventory

The web app is where managers work. Section 2 covers each feature in detail; this is the map of screens and the browser-based timers.

| Area | What lives there | Tag |
| --- | --- | --- |
| Dashboard | Customizable widgets: who is online, time by project, low activity, recent screenshots. | \[D\] |
| Timesheets | View & edit (daily, weekly, calendar), manual time, approvals, manual-time requests. | \[D\] |
| Activity | Screenshots, Apps, URLs, per member and per day, in member, viewer or org timezone. | \[D\] |
| Insights | Utilization, focus, meetings, classification, benchmarks, unusual activity, timeline, highlights. Add-on. | \[D\] |
| Locations | Map, job sites, job-site visits. | \[D\] |
| Schedules | Calendar of shifts (month, week, day), private or shared. | \[D\] |
| Time off | Requests, approvals, balances, holidays. | \[D\] |
| Projects & to-dos | Projects, members, budgets, to-dos, integration links. | \[D\] |
| Clients | Contacts, budgets, invoicing settings. | \[D\] |
| Financials | Payroll, payments, amounts owed, client invoices, team invoices, expenses. | \[D\] |
| Reports | See section 6. | \[D\] |
| People | Members, invites, teams, roles, pay rates, limits. | \[D\] |
| Settings | Organization, members, schedules, activity & tracking, Insights, policies, integrations, billing, security (2FA, SSO, SCIM). | \[D\] |
| Web timer | Tracks time only. No screenshots, activity, app/URL or idle detection. Needs a live connection; keeps running when the tab closes. | \[D\] |
| Chrome extension | Time, keyboard/mouse activity at 15-second granularity, idle detection, reminders. Screenshots removed in v0.10.1. No app/URL tracking. | \[D\] |
| Firefox add-on | Listed on the download page for time tracking. | \[D\] |
| Chromebook | Listed as supported. A "Chromebook app" sends low-quality screenshots instead of blurred ones, but the platform is otherwise barely documented. | \[V\] |

## 6. Reporting feature inventory

Hubstaff documents 23 standard reports plus Insights and custom reports. All export to CSV, and most to PDF. XLS export is not documented.

| Requested area | Hubstaff report(s) | Export | Scheduled email | Tag |
| --- | --- | --- | --- | --- |
| Timesheets | Time & Activity, Daily Totals (weekly), Work Sessions, Timesheet Approval (CSV only) | CSV, PDF, QuickBooks (2 reports) | Yes (not Approval) | \[D\] |
| Attendance | Shift Attendance, Job Attendance, Job Site Visits, Work Breaks | CSV, PDF | Yes (Visits: no) | \[D\] |
| Activity | Time & Activity (activity %), Custom Time & Activity (pick columns, filter by activity) | CSV, PDF | Yes | \[D\] |
| Applications | Apps & URLs | CSV, PDF | Yes | \[D\] |
| Websites | Apps & URLs | CSV, PDF | Yes | \[D\] |
| Screenshots | No screenshot report. Screenshots are viewed on the Activity page. | — | — | \[D\] |
| Projects | Time & Activity grouped by project, Project Budgets (owner only) | CSV, PDF | Partly | \[D\] |
| Tasks | Time & Activity grouped by to-do | CSV, PDF | Yes | \[I\] |
| Clients | Time & Activity grouped by client, Client Budgets (owner only) | CSV, PDF | Partly | \[D\] |
| Billable hours | Time & Activity with billable filter, Client Invoices, Client Invoices Aging | CSV, PDF | Partly | \[D\] |
| Payroll | Amounts Owed, Payments, Team Invoices, Team Invoices Aging | CSV, PDF | Payments only | \[D\] |
| Productivity | Insights Performance report, Insights widgets | Export exists; format not stated | ? | \[D\] / \[U\] |
| Utilization | Insights Utilization widget | ? | ? | \[D\] / \[U\] |
| Budgets | Project Budgets, Client Budgets | CSV, PDF | No | \[D\] |
| Expenses | Expenses (filter by project, member, category, billing type) | CSV, PDF | Yes | \[D\] |
| Time off | Time Off Balances, Time Off Transactions | CSV, PDF | No | \[D\] |
| Limits | Weekly Limits | CSV, PDF | No | \[D\] |
| Audit | Audit Log ("Hubstaff People" beta): author, time, action, object, member, detail | CSV, PDF | No | \[D\] |
| Manual edits | Manual Time Edits | CSV, PDF | Yes | \[D\] |

**Common report features \[D\]**

- "Me" and "All" tabs, filters, group-by (date, member, project, client).
- Saved customized reports with favorites.
- Scheduled delivery daily, weekly or monthly, pausable. Unlimited scheduled reports on all tiers since Q1 2026. One article says scheduling needs Grow or higher \[V\].
- Reports always use the organization's timezone.
- The Time & Activity report was redesigned in Q1 2026.

## 7. Billing feature inventory

Hubstaff sells per seat with feature-gated tiers and add-ons, and handles billing itself. Prices come from Hubstaff's own 2026 pricing guide, because the live pricing table could not be read \[V\].

| Plan | Annual ($/seat/mo) | Monthly ($/seat/mo) | Key gates | Tag |
| --- | --- | --- | --- | --- |
| Free | 0 | 0 | 1 seat, 100 screenshots/mo, no idle timeout. One source says there is no free plan. | \[V\] |
| Small Business Essentials | $144/yr for 4 members | — | Annual only, no add-ons, no integrations or payroll | \[D\] |
| Starter | 4.99 | 7.00 | 500 screenshots and 500 app/URL records per seat/mo, limited payroll | \[D\] |
| Grow | 7.50 | 9.00 | Idle timeout, 1 integration, budgets, breaks, expenses, 1,500 screenshots | \[D\] |
| Team | 10.00 | 12.00 | Unlimited screenshots and integrations, scheduling, time off, approvals, payroll, API | \[D\] |
| Enterprise | Custom | 25.00 (unverified) | SSO, SCIM, HIPAA, Silent app, Insights and Locations included | \[V\] |

| Add-on | Price | Tag |
| --- | --- | --- |
| Insights | $2.50/seat/mo (3-month trial on Team) | \[D\] |
| Tasks | $2.50/seat/mo (included Grow and up) | \[D\] |
| More screenshots | $2.50/seat/mo | \[D\] |
| Silent app | $2.50/seat/mo | \[D\] |
| Locations (GPS) | $3.33/seat/mo | \[D\] |
| Data retention | $1.67/seat/mo | \[D\] |
| VIP support | $29 or $99 per org/mo | \[D\] |

**Billing mechanics**

| Topic | Behaviour | Tag |
| --- | --- | --- |
| Seat count | Every member except Project Viewers takes a seat, including the owner and inactive members. Minimum 2 seats. | \[D\] |
| Billing cycle | Monthly, quarterly or annual. | \[D\] |
| Trial | 14 days, all features, no card. The organization is suspended when it ends without a plan. | \[D\] |
| Adding seats | Monthly plans: full month's charge, no proration. Quarterly and annual: prorated for the rest of the term. | \[D\] |
| Removing seats | No refund. The freed seat can be reused during the term. | \[D\] |
| Upgrade / downgrade | FAQ: change any time, prorated, billed on the next date. Per-seat article: downgrades take effect at the next cycle. | \[V\] |
| Payment | Credit and debit cards. Other methods not documented. | \[D\] / \[U\] |
| Invoices | PDF billing history, choice of invoice email, sales tax shown. VAT and tax-ID handling not documented. | \[D\] / \[U\] |
| Failed payment | Organization suspended. Payment retried after 2 business days. | \[D\] |
| Cancellation | Effective the day before the next bill. Data is kept and comes back on restore; how long it is kept is not stated. | \[D\] / \[U\] |
| Refunds | 30-day money-back after the first paid plan. Credit notes. | \[D\] |

## 8. Integration inventory

Hubstaff has about 35 native integrations. Most are project-management tools: tasks come in, and tracked time goes back out. Integrations need the Grow plan (1 integration) or Team plan (unlimited).

**Common pattern for project-management tools \[D\].** Connect the account, link projects, map users. Tasks import as to-dos, taking about 5 minutes. Time is written back daily, hourly, after a 1-day delay, when a task completes, or never. Some tools receive real time entries; others get a comment. No integration article mentions webhooks, so syncing looks like scheduled polling \[I\].

| Integration | Category | Auth | Direction | Data synced | Tag |
| --- | --- | --- | --- | --- | --- |
| Hubstaff Tasks | Project mgmt (own product) | Native | Two-way | Projects, tasks, time | \[D\] |
| Jira Cloud | Project mgmt | OAuth 1.0 via Application Link | Two-way | Projects, issues, epics, users; worklogs written back | \[D\] |
| Asana | Project mgmt | OAuth | Two-way | Projects, tasks, subtasks, users; time as comments | \[D\] |
| Trello | Project mgmt | OAuth (per user) | Two-way | Boards, cards; time as comments | \[D\] |
| ClickUp | Project mgmt | OAuth | Two-way | Spaces, folders, tasks, users; time as comments | \[D\] |
| Monday.com | Project mgmt | OAuth | Two-way | Boards, items, subitems; time as comments | \[D\] |
| GitHub | Dev / project mgmt | OAuth (per user) | Two-way | Repos, assigned issues; time as comments (org accounts only) | \[D\] |
| GitHub for Insights | Dev analytics | ? | In | PR throughput, review timing | \[D\] / \[U\] |
| GitLab | Dev / project mgmt | OAuth (cloud), app ID + secret (self-hosted) | Two-way | Projects, issues; time into GitLab time tracking | \[D\] |
| Wrike | Project mgmt | ? | Time write-back | Tasks, time entries | \[D\] / \[U\] |
| ActiveCollab, Breeze, LiquidPlanner, Mavenlink, Paymo, Podio, Redbooth, Redmine, Teamwork, Unfuddle, Zoho Projects | Project mgmt | Not checked per tool | Two-way | Projects, tasks, users, time | \[D\] / \[U\] |
| Zendesk | Help desk | OAuth | Two-way | Assigned tickets; time as internal notes | \[D\] |
| Freshdesk | Help desk | API key | Two-way | Assigned tickets; time logs | \[D\] |
| Salesforce | CRM | OAuth | Two-way | Tasks and opportunities; time in the description | \[D\] |
| Insightly | CRM | ? | ? | ? | \[U\] |
| QuickBooks Online | Accounting | OAuth | Out | Time entries, customer/project and user mapping, optional pay rates | \[D\] |
| QuickBooks Desktop | Accounting | File (IIF export) | Out | Time | \[D\] |
| FreshBooks | Accounting / invoicing | OAuth | Two-way | Projects, tasks, time entries | \[D\] |
| PayPal, Wise, Payoneer, Bitwage | Payroll payouts | Account connection (PayPal likely OAuth) | Out | Payments calculated from hours | \[D\] / \[I\] |
| Gusto | Payroll | OAuth | Out | Hours for hourly US employees only | \[D\] |
| Deel | Payroll | OAuth | Out | Approved hours as pay-as-you-go entries | \[D\] |
| Remote | Payroll | ? | Out | Hours | \[D\] / \[U\] |
| BambooHR | HR | OAuth (after entering subdomain) | In | Employees, time-off policies, requests, balances | \[D\] |
| Slack | Communication | OAuth | Out | Timer start/stop and task-complete messages | \[D\] |
| Google / Microsoft 365 calendar | Calendar | OAuth \[I\] | In | Meetings for Insights | \[D\] |
| Microsoft Teams | Communication | ? | In | Collaboration data for Insights only | \[D\] / \[U\] |
| Zapier | Automation | OAuth | Both | Triggers: shift events, timer, to-dos. Actions: create to-dos and clients. | \[D\] |
| Okta, Azure AD, OneLogin, Google Workspace, ADFS | Identity | SAML (+ SCIM for some) | In | Users, groups (Enterprise) | \[D\] |

**Not found as native integrations \[U\]:** Xero, HubSpot, Linear, Basecamp (legacy?), ADP, Pivotal Tracker, Outlook calendar sync. Google Calendar shift sync is documented only through Zapier.

## 9. API capability inventory

Hubstaff API v2 is a broad REST API on the Team plan or higher, with OAuth 2.0 and webhooks. Its main gaps: activity and screenshot data are read-only, and there is no report resource or webhook list endpoint.

| Area | What is available | Tag |
| --- | --- | --- |
| Base / format | `https://api.hubstaff.com/v2`, JSON, ISO 8601 UTC, numeric IDs. OpenAPI spec published. | \[D\] |
| Authentication | OAuth 2.0 / OpenID Connect (authorization code + PKCE); personal refresh tokens; org access tokens (`hsoat_`). Optional DPoP token binding. | \[D\] |
| Token lifetimes | Access 24 h. Personal refresh token 90 days (sliding). OAuth refresh 30 days (sliding). At most 5 refreshes per hour. | \[D\] |
| Scopes | `openid profile email`, `hubstaff:read`, `hubstaff:write`, `tasks:read`, `tasks:write`. | \[D\] |
| Organizations | List, get, create, update. | \[D\] |
| Users / members | List and get users; create members; bulk update org, project and team membership. Invites CRUD. | \[D\] |
| Teams | Full CRUD. | \[D\] |
| Projects | List, create, get, update. | \[D\] |
| Tasks | Full CRUD. Cannot write to projects linked to Hubstaff Tasks or an integration. The Tasks product has its own v1 API. | \[D\] |
| Clients | List, create, get, update. | \[D\] |
| Time entries | Full CRUD, edit logs, timesheets (approve), manual-time requests (update status). | \[D\] |
| Activity | Read-only: 10-min slots with keyboard/mouse %, daily aggregates, incremental updates. | \[D\] |
| Screenshots | Read-only list and get. Screenshot settings read/write. | \[D\] |
| Apps & URLs | Tool usage (read), tracking settings (read/write). | \[D\] |
| Insights | Work-time classification, focus, activity, idle, meetings, unusual activity, tool categories. | \[D\] |
| Attendance | Schedules (read/write), shifts (read), tracking state, job sites, job-site visits, locations. | \[D\] |
| Time off | Policies and requests (no delete), balances, holidays. Overtime policies. | \[D\] |
| Financial | Client and team invoices, team payments, rates, budgets: read-only. | \[D\] |
| Audit | Audit log entries. | \[D\] |
| Reports | No report resource. Reports must be built from activities and Insights endpoints. | \[I\] |
| Webhooks | Org, project and user scoped. Activation handshake, then HMAC-SHA256 signature (`X-Hook-Signature`). Retries on 5xx; disabled after repeated 404/410. No list endpoint. | \[D\] |
| Webhook events | `timer.start/stop`, `project.create`, `client.create`, `task.create/assign/unassign`, `schedule.*`, `shift.ontime/early/late/abandoned/missed`, `work_break.create`, `billing.occupied_seats`. Activity events on request; `idle_time.start` in beta. No events for time-entry edits. | \[D\] |
| Rate limits | 120 requests/min per token, lower on heavy endpoints; 429 with `Retry-After`. The help center says 1,000/hour, which conflicts. | \[D\] / \[V\] |
| Pagination | Cursor based: `page_start_id`, `page_limit` (default 100, max 500). | \[D\] |
| Sandbox | None documented. | \[U\] |
| Tooling | Official CLI (Homebrew, open source), PHP demo, MCP server (Sept 2026; one page still says early access). | \[D\] / \[V\] |

## 10. AI capability inventory

Hubstaff has no built-in generative AI. Its "workforce intelligence" is rules and thresholds, one ML check for automated input, and (since Sept 2026) data access for outside AI assistants. Its own blog of 14 Sept 2026 puts AI summaries in external tools such as Claude or ChatGPT reading Hubstaff data.

| Capability | What exists | How it works | Tag |
| --- | --- | --- | --- |
| AI analytics / Insights | Insights add-on widgets | Aggregates of activity, app/URL, calendar and GitHub data | \[D\] |
| Focus time | Sessions of 30+ min on one project without interruption | Fixed rule | \[D\] |
| Work-time classification | Core, non-core, unproductive per job type | Admin-set; blog says defaults are automatic per job type. No AI classification documented. | \[D\] / \[I\] |
| AI tools category | Recognizes ChatGPT, Claude, Gemini usage (Q4 2025) | Probably a curated list | \[I\] |
| Anomaly detection | "Unusual activity", 7 types, 3 confidence levels, next-day (up to 36 h) | Thresholds (e.g. activity ≥95% for 30 min; keyboard \~0% while mouse active 50 min) plus an "AI-powered" automated-input detector | \[D\] |
| Suspicious apps | Mouse jigglers, auto-clickers, including background apps | App list matching | \[D\] |
| Highlights | Surfaces behaviour changes | Method not documented | \[D\] / \[U\] |
| Benchmarks / engagement score | Industry and job-role medians from anonymised customer data; score 100 = median | Statistical | \[D\] |
| Burnout / overwork | Marketing claims | Only utilization and the ">5 h high focus" rule are documented | \[I\] |
| Recommendations | Marketing claims "resource rebalancing recommendations" | No engine documented | \[I\] / \[U\] |
| AI summaries / chat / natural-language reports | None built in | Only through external assistants via MCP, API or CLI | \[D\] (absence) |
| MCP server | Read-only, owners and managers: orgs, members, projects, tasks, daily activity, time data | Model Context Protocol | \[D\] / \[V\] availability |
| AI-ready API and CLI | Discoverable schema, open-source CLI | Launched 10 Sept 2026 | \[D\] |

**Opening for Worklume.** Native AI summaries, natural-language questions over time data, AI app/URL classification and explained anomalies are all absent. Each would need a strong privacy design.

## 11. Security / privacy inventory

Hubstaff covers the usual controls — 2FA, SAML SSO, SCIM, audit log, SOC 2 Type II, DPA — but SSO, SCIM and HIPAA are Enterprise only, and several details are undocumented.

| Capability | Hubstaff | Tag |
| --- | --- | --- |
| Login | Email and password. No Google, Apple or Microsoft social sign-in. | \[D\] |
| New-browser check | 6-digit email code for accounts without 2FA; 5-min expiry. | \[D\] |
| MFA / 2FA | Authenticator app, email (one article says SMS), recovery codes. Owners and managers can enforce it with 24 h to comply. | \[D\] / \[V\] |
| SSO | SAML: Okta, Azure, ADFS, OneLogin, Google Workspace, Office 365. Can be required. Up to 10 domains. Enterprise. | \[D\] |
| SCIM | Users and groups every 40 min; disabling in the IdP removes the member. Azure documented, Okta and OneLogin in separate articles. Enterprise. | \[D\] / \[V\] |
| Access control | Fixed roles (section 2.1). Users see only their own data. Custom roles unclear. | \[D\] / \[U\] |
| Audit logs | Report of author, time, action, object, member, detail; CSV/PDF export (Q3 2025). Part of "Hubstaff People" beta. Retention and login events not documented. | \[D\] / \[U\] |
| Data retention | Screenshots 2 weeks–6 months by plan; app/URL 1 month; locations 1 month; time logs 3 years. Add-on extends to 1 year (app/URL) and 6 years (time). | \[D\] |
| Screenshot privacy | On-device blur, per-user notification, screenshots off per member or project. | \[D\] |
| Employee controls | Users see their own dashboard and timesheets; can delete whole activity blocks if allowed. A "Data privacy" setting hides app/URL data from dashboards. Whether users see their own screenshots is not confirmed. | \[D\] / \[U\] |
| What is collected | Screenshots, activity (active or inactive only, no keystrokes), apps, URLs, public IP, router MAC. Nothing after the timer stops. | \[D\] |
| Data deletion | Screenshot and time deletion is permanent. Account self-deletion within 24 h. Org data deletion requested by the owner (controller). | \[D\] |
| Data export | Reports as CSV/PDF, member lists, timesheets. No full-org or bulk screenshot export documented. | \[D\] / \[U\] |
| Compliance | SOC 2 Type II (dated 2021; current status not stated), GDPR, CCPA, LGPD, HIPAA (Enterprise), EU–US Data Privacy Framework, DPA with SCCs (April 2026). | \[D\] / \[V\] |
| Encryption | In transit and at rest; algorithms not stated. | \[D\] / \[U\] |
| Hosting | AWS and CloudFront implied by firewall guidance. Region and data residency not documented. | \[I\] / \[U\] |
| Operations | Multi-location failover, continuous backups with point-in-time restore, bug bounty, breach notice within 24 h. | \[D\] |

## 12. Platform capability matrix

Windows and X11 allow nearly everything without prompts. macOS allows everything behind user-granted permissions. Wayland blocks global screen capture, input monitoring and active-window lookup by design.

The Hubstaff column is documented behaviour. The four OS columns are our engineering assessment for building Worklume \[I\], to be confirmed by prototypes.

### 12.1 Desktop capabilities by OS

| Capability | macOS | Windows | Ubuntu / X11 | Ubuntu / Wayland | Hubstaff evidence |
| --- | --- | --- | --- | --- | --- |
| Screenshots | **Supported, permission required.** Screen Recording (TCC); recent macOS versions re-prompt periodically. ScreenCaptureKit. | **Supported.** No prompt. Desktop Duplication or Graphics Capture; DRM video shows black. | **Supported.** No prompt (XShm / XGetImage). | **Partial, OS limitation.** Only via xdg-desktop-portal (consent dialog) or compositor-specific APIs. GNOME Shell's screenshot D-Bus has been restricted since GNOME 41. | Mac and Windows full. Wayland: GNOME only, app 1.6.10+; GNOME 42+ needs a workaround or Xorg. \[D\] |
| Multi-monitor capture | Supported | Supported | Supported | Partial: the portal may ask the user to pick screens | All monitors captured. \[D\] |
| Active application | **Supported.** Front app with no permission; window title needs Screen Recording or Accessibility. | **Supported.** Foreground window API. | **Supported.** EWMH `_NET_ACTIVE_WINDOW`. | **Partial / OS limitation.** No standard protocol. GNOME needs a Shell extension; KDE via KWin scripting; wlroots via foreign-toplevel. | App tracking on all 3 OSes; Wayland unknown. \[D\] / \[U\] |
| URL tracking | **Permission required.** Automation (AppleScript) per browser; Accessibility fallback. | **Supported.** UI Automation reads the address bar. | **Partial.** Accessibility (AT-SPI) or a browser extension. | **Partial.** Browser extension is the only reliable route. | Chrome, Firefox, Edge, Safari, Island; macOS needs Automation. \[D\] |
| Keyboard/mouse activity | **Permission required.** Input Monitoring or Accessibility for event taps; seconds-since-last-input needs none. | **Supported.** Low-level hooks or last-input time. | **Supported.** XInput2 / XRecord. | **OS limitation.** No global input capture. Activity can be approximated per second from idle-notify (ext-idle-notify-v1 or Mutter IdleMonitor), without a keyboard/mouse split. | Linux shows one combined figure. \[D\] |
| Idle detection | Supported (no permission) | Supported | Supported | Supported via idle-notify / Mutter / logind | All desktop apps. \[D\] |
| System tray / menu bar | Supported (menu bar item) | Supported (notification area) | Supported (AppIndicator / StatusNotifierItem) | **Partial.** Stock GNOME has no tray; needs the AppIndicator extension (Ubuntu ships it). | Tray on all 3 OSes. \[D\] |
| Auto-start | Supported: login item (SMAppService) or LaunchAgent | Supported: Run key or Task Scheduler | Supported: XDG autostart or systemd user unit | Same as X11 | Windows documented; others unknown. \[D\] / \[U\] |
| Background / silent agent | **Partial.** MDM profile can pre-approve Accessibility and Input Monitoring, but not silently grant Screen Recording. | **Supported.** Per-user agent is required: a Windows service cannot capture the user's desktop. | Supported (systemd user service) | Partial: portal consent still applies | Silent app on all 3 via MDM. macOS mode can drop screenshots to avoid the prompt. \[D\] |
| Offline tracking | Supported (local encrypted store) | Supported | Supported | Supported | All desktop apps. \[D\] |
| Notifications | Permission required | Supported (needs app ID registration) | Supported (libnotify) | Supported | Screenshot notifications on all 3. \[D\] |
| Auto-update | **Supported.** Sparkle-style; code signing and notarization required. | **Supported.** Signed installer / MSIX; MDM may disable. | **Partial.** apt repo, AppImage or self-updater; Flatpak and Snap sandboxes would block monitoring. | Same as X11 | Opt-in auto-install; macOS unattended updates not allowed. \[D\] / \[V\] |
| Packaging | .dmg / .pkg, notarized | .exe / .msi | .deb, .rpm, AppImage | Same | .sh only on Linux. \[D\] |

**Takeaways for Worklume**

- Treat Wayland as a separate target with a reduced feature set, shown clearly in the UI and in sales material.
- On macOS, onboarding must walk users through 3–4 permission prompts and detect when they are revoked.
- On Windows, split the agent into a per-user tracking process and an optional service for updates and tamper-resistance.

### 12.2 Master feature matrix

✓ = this surface is involved. Complexity is for Worklume to build.

| Domain | Feature | Hubstaff | Web | Desktop | Mobile | Backend | Integration | Complexity |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Organization | Orgs, members, invites | Yes \[D\] | ✓ |  | ✓ | ✓ |  | LOW |
| Organization | Roles and permissions (org, project, team) | Yes \[D\] | ✓ | ✓ | ✓ | ✓ |  | MEDIUM |
| Organization | Policy hierarchy (org default, member override) | Yes \[D\] | ✓ | ✓ | ✓ | ✓ |  | MEDIUM |
| Organization | Teams and team leads | Yes \[D\] | ✓ |  |  | ✓ |  | MEDIUM |
| Organization | Custom roles | Unclear \[U\] | ✓ |  |  | ✓ |  | MEDIUM |
| Time | Desktop timer | Yes \[D\] |  | ✓ |  | ✓ |  | MEDIUM |
| Time | Web timer / browser extension | Yes \[D\] | ✓ |  |  | ✓ |  | LOW |
| Time | Mobile timer | Yes \[D\] |  |  | ✓ | ✓ |  | MEDIUM |
| Time | Manual time with reasons and approvals | Yes \[D\] | ✓ | ✓ | ✓ | ✓ |  | MEDIUM |
| Time | Timesheets and approval workflow, locking | Yes \[D\] | ✓ |  | ✓ | ✓ |  | HIGH |
| Time | Idle detection and resolution | Yes \[D\] |  | ✓ |  | ✓ |  | MEDIUM |
| Time | Offline tracking and sync | Yes \[D\] |  | ✓ | ✓ | ✓ |  | HIGH |
| Time | Multi-device timer sync | Unknown \[U\] | ✓ | ✓ | ✓ | ✓ |  | HIGH |
| Time | Limits and automatic start | Yes \[D\] | ✓ | ✓ | ✓ | ✓ |  | MEDIUM |
| Tracking | Screenshots (capture, blur, upload) | Yes \[D\] | ✓ | ✓ |  | ✓ |  | VERY HIGH |
| Tracking | Activity % | Yes \[D\] | ✓ | ✓ |  | ✓ |  | HIGH |
| Tracking | App and URL tracking | Yes \[D\] | ✓ | ✓ |  | ✓ |  | VERY HIGH |
| Tracking | Silent / MDM-deployed agent | Yes \[D\] |  | ✓ |  | ✓ |  | HIGH |
| Tracking | Wayland support | Partial \[D\] |  | ✓ |  |  |  | VERY HIGH |
| Mobile | GPS tracking modes | Yes \[D\] | ✓ |  | ✓ | ✓ |  | HIGH |
| Mobile | Geofenced job sites, auto clock-in | Yes \[D\] | ✓ |  | ✓ | ✓ |  | HIGH |
| Projects | Projects, to-dos, clients | Yes \[D\] | ✓ | ✓ | ✓ | ✓ |  | LOW |
| Projects | Budgets and alerts, auto-stop | Yes \[D\] | ✓ | ✓ |  | ✓ |  | MEDIUM |
| Projects | Pay and bill rates with effective dates | Yes \[D\] | ✓ |  |  | ✓ |  | MEDIUM |
| Projects | Kanban task product | Yes \[D\] | ✓ |  | ✓ | ✓ |  | HIGH |
| Workforce | Schedules and shifts, shift status | Yes \[D\] | ✓ | ✓ | ✓ | ✓ |  | HIGH |
| Workforce | Breaks | Yes \[D\] | ✓ | ✓ | ✓ | ✓ |  | MEDIUM |
| Workforce | Time off, accruals, holidays | Yes \[D\] | ✓ |  | ✓ | ✓ | ✓ | HIGH |
| Workforce | Overtime | Yes \[D\] | ✓ |  |  | ✓ |  | MEDIUM |
| Productivity | Classification by job type | Yes \[D\] | ✓ |  |  | ✓ |  | MEDIUM |
| Productivity | Insights (focus, utilization, meetings, benchmarks) | Yes \[D\] | ✓ |  |  | ✓ | ✓ | HIGH |
| Productivity | Unusual activity detection | Yes \[D\] | ✓ | ✓ |  | ✓ |  | HIGH |
| Productivity | Office vs remote detection | Yes \[D\] | ✓ | ✓ |  | ✓ |  | MEDIUM |
| Reports | Standard reports, filters, export | Yes \[D\] | ✓ |  |  | ✓ |  | HIGH |
| Reports | Saved and scheduled reports | Yes \[D\] | ✓ |  |  | ✓ |  | MEDIUM |
| Financial | Payroll and payouts | Yes \[D\] | ✓ |  |  | ✓ | ✓ | VERY HIGH |
| Financial | Client and team invoices | Yes \[D\] | ✓ |  |  | ✓ | ✓ | HIGH |
| Financial | Expenses with receipts | Yes \[D\] | ✓ |  | ✓ | ✓ |  | MEDIUM |
| Billing | Subscriptions, seats, proration, add-ons | Yes \[D\] | ✓ |  |  | ✓ | ✓ | HIGH |
| Integrations | Project-management two-way sync | Yes \[D\] | ✓ | ✓ |  | ✓ | ✓ | HIGH |
| Integrations | Accounting, payroll, HR, Slack, Zapier | Yes \[D\] | ✓ |  |  | ✓ | ✓ | HIGH |
| Platform | Public API, OAuth, webhooks | Yes \[D\] | ✓ |  |  | ✓ | ✓ | HIGH |
| Platform | MCP server / AI data access | Yes \[D\] |  |  |  | ✓ | ✓ | MEDIUM |
| AI | Native AI summaries and Q&A | No \[D\] | ✓ |  | ✓ | ✓ |  | HIGH |
| Security | 2FA, SSO (SAML), SCIM | Yes \[D\] | ✓ | ✓ | ✓ | ✓ | ✓ | HIGH |
| Security | Audit log | Yes \[D\] | ✓ |  |  | ✓ |  | MEDIUM |
| Security | Retention policies and deletion | Yes \[D\] | ✓ | ✓ |  | ✓ |  | HIGH |

## 13. Worklume feature inventory

Worklume needs 14 modules to reach Hubstaff's scope. The right-hand column names where Worklume can do better; everything in it is a proposal, not a Hubstaff fact.

| # | Module | Worklume features | Where Worklume can differ |
| --- | --- | --- | --- |
| 1 | Identity & organization | Organizations, multi-org membership with switcher, members, invites (single and bulk), teams, org/project/team roles, custom roles, policy hierarchy (org → team → member) | Custom roles from day one; team-level policy layer (Hubstaff has org and member only) |
| 2 | Time engine | Desktop/web/mobile timers, manual time with reasons, edit logs, idle resolution, limits, automatic start, offline queue, one active timer per user across devices, billable flag, time zones | Explicit multi-device rule; optional rounding rules |
| 3 | Timesheets & approvals | Daily, weekly, calendar views; manual-time requests; submit/approve/deny; locking; bulk approval; reopen | Configurable multi-step approval chains |
| 4 | Desktop agent | Screenshots (frequency, blur, multi-monitor, notification), activity %, app/URL tracking, idle, tray, auto-start, auto-update, offline store, silent/MDM mode, permission health checks | Employee "my data" view in the agent; per-OS capability report shown to admins; honest Wayland mode |
| 5 | Mobile apps | Timer, GPS modes, geofenced job sites, auto clock-in/out, shifts, breaks, time off requests, push notifications, offline | Battery-aware tracking profiles |
| 6 | Projects & clients | Projects, to-dos, clients, members, project budgets and client budgets (hours/cost), alerts, auto-stop, pay/bill rates with effective dates, per-project rates | Per-project bill rates (not documented in Hubstaff) |
| 7 | Workforce management | Schedules, recurring shifts, shift status and grace, breaks, time off with accruals, holidays by country, overtime (weekly and daily), adherence metric | Daily overtime and double-time rules; schedule adherence score |
| 8 | Productivity & analytics | Activity dashboards, app/URL classification by job type, focus time, utilization, meetings (calendar), benchmarks (later), unusual-activity rules | Transparent rules shown to employees |
| 9 | Screenshots & evidence | Viewer, access by role, delete (keep or remove time), retention policies per plan and per org, audit trail of views | Screenshot-view audit; per-org retention shorter than plan maximum |
| 10 | Reporting | Standard reports (time, activity, apps/URLs, attendance, projects, clients, budgets, payroll, invoices, expenses, time off, limits, audit), custom columns, saved reports, scheduled email, CSV/PDF/XLSX | XLSX export; report API |
| 11 | Financials | Payroll calculation, payouts via providers, payslips, client invoices, contractor invoices, expenses with receipts, categories and approvals | Expense approvals and categories (gaps in Hubstaff docs) |
| 12 | Billing (Worklume's own) | Plans, seats, trials, add-ons, proration, invoices, taxes/VAT, dunning, suspension | Clear, published proration rules |
| 13 | Integrations & platform | Public REST API, OAuth 2.0, org tokens, webhooks (incl. time-entry events and list endpoint), Zapier, Slack/Teams, Jira/Asana/ClickUp/GitHub, QuickBooks/Xero, BambooHR, calendar, MCP server | Time-entry webhooks; webhook list endpoint; Xero and MS Teams |
| 14 | Security, privacy & AI | 2FA, SSO, SCIM, audit log, retention, export, deletion, consent records; AI summaries, natural-language questions, AI classification, explained anomalies | Native AI with privacy controls; full organization data export |

## 14. MVP / V1 / V2 / V3 / Future classification

Every Hubstaff feature is kept. Phases are ordered by dependency: the tracking core comes first because everything else rolls up from it. Nothing is dropped for being hard; hard items are placed where their dependencies are ready.

&#91;embedded content: Worklume roadmap · 5 phases, 4 gates\]

Each phase ends at a gate: the next phase starts only once its criterion holds.

### MVP

| Feature | Complexity | Depends on |
| --- | --- | --- |
| Organizations, members, invites, owner/manager/user roles | LOW | — |
| Projects, to-dos, clients | LOW | Organizations |
| Policy hierarchy (org default + member override) | MEDIUM | Organizations |
| Desktop agent, Windows + macOS: timer, tray, auto-start, auto-update | HIGH | Code signing, update server |
| Screenshots with frequency, blur, multi-monitor, notification | VERY HIGH | Agent, object storage |
| Activity % per 10-min slot | HIGH | Agent |
| Idle detection and resolution | MEDIUM | Agent |
| Offline queue and sync | HIGH | Agent, slot model |
| Web timer | LOW | Time engine |
| Manual time with reasons, edit log | MEDIUM | Time engine |
| Timesheets (daily, weekly) | MEDIUM | Time engine |
| Dashboard + core reports: Time & Activity, Daily Totals, Manual Edits, Screenshots view | HIGH | Slot model |
| CSV/PDF export | MEDIUM | Reports |
| Screenshot deletion + plan retention | MEDIUM | Storage |
| Subscription billing: plans, seats, trial, invoices | HIGH | Billing provider |
| Email/password login, email verification | LOW | — |

### V1

| Feature | Complexity | Depends on |
| --- | --- | --- |
| App and URL tracking (Windows, macOS) | VERY HIGH | Agent |
| Ubuntu X11 agent (.deb) | HIGH | Agent core |
| Teams, team leads, project roles | MEDIUM | Roles |
| Timesheet approvals and locking; manual-time approvals | HIGH | Timesheets |
| Limits and automatic start | MEDIUM | Policies |
| Pay and bill rates with effective dates | MEDIUM | Members |
| Project and client budgets with alerts | MEDIUM | Rates |
| Client invoices | HIGH | Rates |
| Mobile timer (iOS, Android), offline | MEDIUM | Time engine, API |
| Public REST API, OAuth 2.0, org tokens, webhooks | HIGH | Stable domain model |
| Jira, Asana, ClickUp, GitHub, Slack, Zapier | HIGH | API, sync engine |
| 2FA (TOTP), enforcement | MEDIUM | Auth |
| Saved and scheduled reports; Apps & URLs report | MEDIUM | Reports |
| Seat changes, proration, add-ons, dunning | HIGH | Billing |

### V2

| Feature | Complexity | Depends on |
| --- | --- | --- |
| Schedules, recurring shifts, shift status, attendance reports | HIGH | Time engine, time zones |
| Breaks (paid/unpaid policies) | MEDIUM | Timer |
| Time off, accruals, holidays by country | HIGH | Schedules |
| Overtime (weekly, daily, multipliers) | MEDIUM | Rates, approvals |
| Payroll calculation, payslips, payouts (Wise, PayPal, Deel, Gusto) | VERY HIGH | Approvals, rates, overtime, time off |
| Contractor (team) invoices | MEDIUM | Payroll |
| Expenses with receipts, categories, approvals | MEDIUM | Projects, storage |
| GPS tracking modes, geofenced job sites, auto clock-in | HIGH | Mobile |
| QuickBooks, Xero, BambooHR, calendar sync | HIGH | Integrations engine |
| Full report inventory (budgets, payroll, time off, invoices aging) | HIGH | All above |

### V3

| Feature | Complexity | Depends on |
| --- | --- | --- |
| SAML SSO, SCIM | HIGH | Auth |
| Audit log with export | MEDIUM | Event log |
| Silent / MDM-deployed agent | HIGH | Agent, MDM packaging |
| Custom retention, org data export, deletion workflows | HIGH | Storage, jobs |
| Custom roles | MEDIUM | Permission model |
| Productivity classification by job type | MEDIUM | App/URL data |
| Focus time, utilization, meetings | HIGH | Classification, calendar |
| Unusual activity rules and suspicious-app detection | HIGH | Activity data |
| Office vs remote detection | MEDIUM | Agent network data |
| AI daily/weekly summaries for managers and employees | HIGH | Analytics, privacy controls |

### Future

| Feature | Complexity | Depends on |
| --- | --- | --- |
| Wayland agent (GNOME first, then KDE) | VERY HIGH | Linux agent, portals |
| Industry benchmarks, engagement score | HIGH | Large anonymised dataset |
| Natural-language questions over workforce data; MCP server | HIGH | API, analytics |
| AI app/URL classification, explained anomalies | HIGH | Classification data |
| Kanban task product | HIGH | Projects |
| Chromebook / browser-extension tracking | MEDIUM | Web timer |
| Field service: work orders, routes | HIGH | GPS |
| Integration marketplace, reseller program | MEDIUM | API |
| Data residency (EU region) | HIGH | Infrastructure |

## 15. Technical risk register

Five risks rate VERY HIGH. Four of them sit in the desktop agent or in payroll; the fifth is privacy law. All five need prototypes or legal review before Phase 2 design is locked. Severity is our assessment.

| # | Risk | Why it is hard | Severity | Mitigation to explore |
| --- | --- | --- | --- | --- |
| 1 | macOS permissions | Screen Recording, Accessibility, Input Monitoring and per-browser Automation are separate user prompts. Users can revoke them silently. Recent macOS versions re-prompt for screen recording. MDM cannot silently grant Screen Recording. | VERY HIGH | Guided onboarding, a live permission health check, and "degraded" states reported to admins |
| 2 | Linux Wayland | No global screen capture, input capture or active-window API. Each compositor differs (GNOME, KDE, wlroots). GNOME restricted its screenshot D-Bus from 41. Portals show consent dialogs. Even Hubstaff supports only GNOME screenshots. | VERY HIGH | Reduced feature set; portals; per-compositor adapters; idle-notify for activity; publish a support matrix |
| 3 | Screenshot capture | High-DPI and multi-monitor handling, DRM black frames, CPU spikes, random timing per slot, on-device blur, and never capturing when idle or stopped. | HIGH | Native capture per OS, downscale + compress on device, capture budget per slot |
| 4 | App/URL tracking | Reading the URL differs per browser and OS (AppleScript, UI Automation, AT-SPI) and breaks when browsers update. Private windows and incognito need a policy. | VERY HIGH | Browser-specific adapters, optional extension, automated tests on browser betas |
| 5 | Offline synchronization | Slots can be recorded offline for days, then uploaded late, out of order or twice. They may overlap manual edits, locked timesheets or project changes made meanwhile. | HIGH | Idempotent slot IDs, server-side conflict rules, encrypted local queue, clock-skew handling |
| 6 | Multi-device timers | A user can start a timer on laptop and phone. Rules are needed for one active timer, handover and overlapping time. Hubstaff does not document its rule. | HIGH | One active timer per user, enforced by the server with real-time push |
| 7 | Privacy and legal | Monitoring laws differ (GDPR, works councils in Germany, US state laws, India DPDP Act). Consent, notice, access requests and retention must be provable. | VERY HIGH | Privacy-by-design defaults, employee data view, consent records, legal review per market |
| 8 | Screenshot storage at scale | Up to 18 images per hour per member (3 per 10 min × monitors). Storage, bandwidth and deletion jobs grow fast; retention deletes must be reliable. | HIGH | Object storage with lifecycle rules, thumbnails, per-plan caps, cost model per seat |
| 9 | Reporting at scale | Reports aggregate millions of 10-min slots across time zones, roles and filters, and must match payroll to the cent. | HIGH | Pre-aggregated daily rollups, separate analytics store, one timezone rule set |
| 10 | Payroll and payouts | Money movement, provider APIs, currencies, tax rules, failed payouts, and reconciliation with approved timesheets. | VERY HIGH | Start with calculate-and-export; add payouts through providers only; never hold funds |
| 11 | Subscription billing | Seat counting, proration, add-ons, taxes/VAT, dunning and suspension. Hubstaff's own docs conflict on proration. | HIGH | Use a billing platform (e.g. Stripe Billing); write proration rules down before build |
| 12 | Mobile GPS | Background location is restricted by iOS and Android and costs battery. OEM battery savers kill apps. Store review scrutinises location use. | HIGH | Significant-change and geofence APIs, clear disclosures, tracking only in declared modes |
| 13 | AI analytics | Needs clean classified data first. Wrong "unproductive" or "fraud" labels harm employees. LLM summaries of monitoring data raise privacy and cost issues. | HIGH | Rules first, explainable outputs, human review, opt-in AI, no training on customer data |
| 14 | Agent distribution and updates | Code signing, macOS notarization, Windows SmartScreen reputation, Linux packaging, staged rollouts and rollback. | HIGH | Signed builds from day one, update channels, crash reporting |
| 15 | Tamper and fraud resistance | Mouse jigglers, auto-clickers, VM use, clock changes, killing the agent. | MEDIUM | Suspicious-app list, input-pattern checks, heartbeat gaps flagged to admins |
| 16 | Integration upkeep | 30+ third-party APIs with different auth, limits and data models; two-way sync conflicts. | MEDIUM | A shared sync engine with per-connector adapters; launch with few connectors |

## 16. Unknowns / requires validation

These 22 points could not be settled from official sources. Most are best settled by using a Hubstaff trial account, not by more reading.

| # | Question | Why it matters | Status | How to settle |
| --- | --- | --- | --- | --- |
| 1 | Default idle timeout: 5 or 20 minutes? | Default policy values | \[V\] conflict | Trial account |
| 2 | Can a timer run on two devices at once? | Multi-device design | \[U\] | Trial, two devices |
| 3 | Does macOS need Accessibility or Input Monitoring? | Onboarding flow | \[U\] | Install on macOS |
| 4 | App/URL and activity tracking on Wayland; KDE support | Linux scope | \[U\] | Test on Ubuntu 24.04 GNOME and Kubuntu |
| 5 | Any .deb/.rpm package or distros beyond Ubuntu | Linux packaging | \[U\] | Download page, support ticket |
| 6 | Auto-start on macOS and Linux | Agent behaviour | \[U\] | Install test |
| 7 | Configurable per-member low-activity alert | Alert design | \[U\] | Trial account |
| 8 | Live pricing and Enterprise $25 monthly figure | Pricing strategy | \[V\] | Check pricing page by hand |
| 9 | Is there still a Free plan? | Pricing strategy | \[V\] conflict | Pricing page, signup |
| 10 | Proration on mid-cycle downgrade | Billing rules | \[V\] conflict | Support ticket |
| 11 | Expenses plan tier (Grow or Team); approval workflow; categories | Expense scope | \[V\] / \[U\] | Trial account |
| 12 | How custom roles work | Permission model | \[U\] | Trial (Enterprise demo) |
| 13 | 2FA methods: SMS or email? | Auth design | \[V\] conflict | Trial account |
| 14 | SCIM IdPs: Azure only, or also Okta and OneLogin? | Enterprise scope | \[V\] conflict | Enterprise sales call |
| 15 | Is SOC 2 Type II current in 2026? | Competitive position | \[V\] | Trust center request |
| 16 | API rate limit: 120/min or 1,000/hour? | API design benchmark | \[V\] conflict | Developer portal test |
| 17 | MCP server: generally available or early access? | AI positioning | \[V\] conflict | Help center, sales |
| 18 | How default app/URL classifications are generated | Classification design | \[U\] | Trial with Insights |
| 19 | Can employees see their own screenshots? | Privacy features | \[U\] | Trial as a user role |
| 20 | Data retention after cancellation; purge timing | Retention policy | \[U\] | Support ticket |
| 21 | Hosting region and data residency | Enterprise and EU sales | \[U\] | Trust center |
| 22 | Chromebook app capture behaviour | Platform scope | \[V\] | Install on Chromebook |

## 17. Recommended next research

The next step is hands-on: a Hubstaff trial plus three agent prototypes will settle most unknowns and the top risks. Only then should Phase 2 (data model and architecture) begin.

1. **Hubstaff hands-on trial (1–2 weeks).** Team plan with Insights, on macOS, Windows, Ubuntu X11 and Ubuntu Wayland, plus iOS and Android. Work through the section 16 list and record every settings screen.
2. **Desktop agent spikes.**
   - macOS: ScreenCaptureKit screenshot, per-second input sampling, Safari and Chrome URL reading, and the permission prompts each one triggers.
   - Windows: capture, UI Automation URL reading, per-user agent vs service.
   - Ubuntu 24.04 Wayland: portal screenshot, GNOME extension for the active window, idle-notify for activity.
   - Pick the agent stack (native per OS vs Rust/Tauri/Electron core) from the results.
3. **Offline and multi-device sync design.** Write the conflict rules for late, duplicate and overlapping slots before any schema work.
4. **Legal and privacy review.** Employee-monitoring rules for launch markets (India DPDP Act, GDPR and works councils, US states). Output: default settings, consent text, retention defaults.
5. **Storage cost model.** Screenshot size × frequency × monitors × retention, per seat per plan, to set caps and pricing.
6. **Competitor scan.** Time Doctor, ActivTrack, Insightful, Clockify, Toggl Track, DeskTime. Focus on Linux/Wayland support, AI features and privacy stance, to sharpen Worklume's positioning.
7. **Pricing and packaging.** Decide Worklume tiers and add-ons once cost and competitor data are in.
8. **Customer interviews.** 8–10 target buyers (agencies, BPOs, remote-first teams) to rank V1 vs V2 features.

### Sources

All opened on 28 Sept 2026. Grouped by topic; the help center is support.hubstaff.com.

**Platforms and desktop:** [Supported OS](https://support.hubstaff.com/supported-operating-systems-browsers-devices/) · [Wayland screenshots](https://support.hubstaff.com/screenshot-capture-support-wayland-linux/) · [Linux install](https://support.hubstaff.com/install-and-sign-in-into-linux-desktop-client/) · [Automated updates](https://support.hubstaff.com/automated-updates) · [Silent app (Linux)](https://support.hubstaff.com/hubstaff-silent-app-setup-linux/) · [Tracking by app](https://support.hubstaff.com/tracking-functionalities-various-apps-clients/) · [App & URL tracking](https://support.hubstaff.com/application-url-tracking/) · [macOS URL permissions](https://support.hubstaff.com/how-to-enable-app-url-tracking-for-osx/) · [macOS screen permission](https://support.hubstaff.com/how-to-give-hubstaff-screen-capture-permissions-on-macos/) · [Activity calculation](https://support.hubstaff.com/how-are-activity-levels-calculated/) · [Idle warning](https://support.hubstaff.com/how-does-the-inactivity-warning-work/) · [Keep idle time](https://support.hubstaff.com/keep-idle-time-organization-setting/) · [Preferences](https://support.hubstaff.com/hubstaff-preferences-settings/) · [Check updates](https://support.hubstaff.com/check-updates-hubstaff-app/) · [Automatic start](https://support.hubstaff.com/what-is-automatic-start/) · [What desktop tracks](https://support.hubstaff.com/what-does-the-desktop-application-track/) · [Offline tracking](https://hubstaff.com/time-tracking/accurate-offline-time-tracking) · [Download](https://hubstaff.com/download) · [Web timer](https://support.hubstaff.com/web-timer/)

**Screenshots:** [Frequency](https://support.hubstaff.com/change-screenshot-frequency/) · [Disable](https://support.hubstaff.com/how-to-disable-screenshots-hubstaff/) · [Dual monitors](https://support.hubstaff.com/hubstaff-offer-dual-monitor-support/) · [Blur](https://support.hubstaff.com/screenshot-blur/) · [Notifications](https://support.hubstaff.com/turn-screenshot-notifications/) · [Delete](https://support.hubstaff.com/how-to-delete-screenshots/) · [Data retention](https://support.hubstaff.com/data-retention-comparison/)

**Mobile:** [Location tracking](https://support.hubstaff.com/hubstaff-mobile-app-location-tracking/) · [Job sites](https://support.hubstaff.com/job-sites/) · [Shift status](https://support.hubstaff.com/shift-marked-late-missed-abandoned/) · [Field tracking](https://hubstaff.com/field-tracking-software) · [App Store](https://apps.apple.com/us/app/hubstaff-time-tracker/id971894122) · [Google Play](https://play.google.com/store/apps/details?id=com.netsoft.Hubstaff)

**Web app and workforce:** [Roles & permissions](https://support.hubstaff.com/hubstaff-user-rolepermissions-guide/) · [Teams](https://support.hubstaff.com/teams-overview/) · [Organization settings](https://support.hubstaff.com/organization-settings/) · [Members & invites](https://support.hubstaff.com/members-invites-overview/) · [Manual time](https://support.hubstaff.com/how-to-add-delete-and-edit-manual-time-entries/) · [Manual time approvals](https://support.hubstaff.com/manual-time-approvals/) · [Timesheet approvals](https://support.hubstaff.com/team-members-guide-to-timesheet-approvals/) · [Locked time](https://support.hubstaff.com/locked-time-in-hubstaff/) · [Time zones](https://support.hubstaff.com/viewing-data-different-time-zones/) · [Limits](https://support.hubstaff.com/user-limits/) · [Projects](https://support.hubstaff.com/projects-overview/) · [Clients](https://support.hubstaff.com/clients-overview/) · [Project budgets](https://support.hubstaff.com/project-level-budgets/) · [Schedules](https://support.hubstaff.com/schedules/) · [Work breaks](https://support.hubstaff.com/work-breaks/) · [Time off & holidays](https://support.hubstaff.com/paid-time-off-holidays-and-time-off-requests/) · [Overtime](https://support.hubstaff.com/overtime/) · [Payroll](https://support.hubstaff.com/payroll-overview/) · [Invoices](https://support.hubstaff.com/creating-invoice-overview/) · [Expenses](https://support.hubstaff.com/expenses/) · [Reports overview](https://support.hubstaff.com/reports-overview/) · [Export reports](https://support.hubstaff.com/export-or-download-reports/) · [Dashboard](https://support.hubstaff.com/web-application-dashboard-overview-owners/)

**Billing:** [Pricing](https://hubstaff.com/pricing) · [Pricing guide](https://hubstaff.com/hubstaff-pricing-guide) · [Plans and pricing](https://support.hubstaff.com/new-pricing-and-plans/) · [Per-seat pricing](https://support.hubstaff.com/per-seat-pricing/) · [Proration policy](https://hubstaff.com/pro-ration-policy) · [Add-ons](https://support.hubstaff.com/hubstaff-add-ons/) · [Subscription invoice](https://support.hubstaff.com/how-to-read-your-hubstaff-subscriptions-invoice/) · [FAQs](https://support.hubstaff.com/hubstaff-faqs/) · [Archive organization](https://support.hubstaff.com/archive-an-organization/)

**Integrations and API:** [Integrations](https://hubstaff.com/integrations) · [Integrations overview](https://support.hubstaff.com/integrations-overview/) · [Task write-back](https://support.hubstaff.com/hubstaff-integration-task-write-back/) · [Jira](https://support.hubstaff.com/jira-time-tracking-setup/) · [QuickBooks Online](https://support.hubstaff.com/quickbooks-online-integration-setup/) · [BambooHR](https://support.hubstaff.com/bamboohr-integration-setup/) · [Zapier](https://support.hubstaff.com/zapier-integration-setup-hubstaff/) · [Developer portal](https://developer.hubstaff.com/) · [Authentication](https://developer.hubstaff.com/authentication) · [Rate limits](https://developer.hubstaff.com/pagination-rate-limits/) · [Webhooks](https://developer.hubstaff.com/webhooks/) · [API v2 spec](https://api.hubstaff.com/v2/docs) · [API FAQ](https://support.hubstaff.com/hubstaff-api-faq/) · [CLI](https://support.hubstaff.com/using-the-hubstaff-cli/)

**AI:** [Insights](https://hubstaff.com/insights) · [Insights add-on](https://support.hubstaff.com/hubstaff-insights-add-on/) · [Unusual activity](https://support.hubstaff.com/hubstaff-insights-unusual-activity/) · [MCP server](https://support.hubstaff.com/hubstaff-mcp-server/) · [Workforce intelligence](https://hubstaff.com/workforce-intelligence) · [What's new](https://hubstaff.com/blog/what-is-new/) · [CLI & AI-ready API](https://hubstaff.com/blog/hubstaff-cli-ai-ready-api/) · [AI workflows](https://hubstaff.com/blog/ai-workflows-hubstaff-data/) · [Press release, 10 Sept 2026](https://www.prnewswire.com/news-releases/hubstaff-launches-ai-ready-workforce-data-layer-enabling-ai-agents-to-query-and-act-on-workforce-data-directly-302875574.html)

**Security and privacy:** [Security](https://hubstaff.com/security) · [SSO setup](https://support.hubstaff.com/configuring-single-sign-on/) · [SSO & SCIM](https://hubstaff.com/sso-scim) · [SCIM](https://support.hubstaff.com/configuring-system-for-cross-domain-identity-management-scim-integration/) · [2FA](https://support.hubstaff.com/turn-on-two-factor-authentication-2fa/) · [Audit log](https://support.hubstaff.com/audit-log-report/) · [Data handling FAQ](https://support.hubstaff.com/data-handling-and-protection-faq/) · [Delete account](https://support.hubstaff.com/how-to-delete-your-hubstaff-account/) · [Remote vs office](https://support.hubstaff.com/how-to-set-up-remote-vs-office-tracking-in-hubstaff/) · [SOC 2](https://hubstaff.com/soc-2) · [Privacy policy](https://hubstaff.com/privacy) · [DPA](https://hubstaff.com/dpa)

Platform assessments in section 12 (macOS, Windows, X11 and Wayland APIs) are our engineering judgement, not Hubstaff sources, and must be checked in the prototypes above.
