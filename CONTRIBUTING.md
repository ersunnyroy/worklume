# Contributing

## One story per change

Each pull request implements one story from the implementation plan
([docs/planning/phase-3-epics-and-stories.md](docs/planning/phase-3-epics-and-stories.md)).
Unrelated fixes go in their own pull request.

Every story follows the same loop, and the pull request shows each step:

1. **Research** the existing code, the affected modules and the plan.
2. **Plan** the change: database, API, authorization and tenant-isolation impact, and tests.
3. **Implement** in the owning module.
4. **Test**, including tenant-isolation tests for anything that touches tenant data.
5. **Review** security, tenant isolation and performance, and write the notes in the PR.
6. **Document** decisions and behaviour.
7. **Commit** and open the pull request.

## Branches

`<type>/<story>-<short-name>`, for example `feat/E00-S01-monorepo` or
`fix/E08-S02-timer-overlap`. The type matches the commit type.

## Commits

Commits follow [Conventional Commits](https://www.conventionalcommits.org/) and are checked
by commitlint (`pnpm commitlint --edit <file>`, or in CI).

```
feat(api): add organization invitations

Explain what changed and why, not how.

Refs: E02-S03
```

- **Types:** `feat`, `fix`, `refactor`, `perf`, `test`, `docs`, `build`, `ci`, `chore`, `style`, `revert`.
- **Scopes:** `repo`, `api`, `web`, `desktop`, `mobile`, `ext`, `infra`, `docs`, `deps`, `ci`,
  `config`.
- **Footer:** `Refs: <story ID>` on every commit that belongs to a story.

## Definition of Done

A story is done only when every applicable line of the Definition of Done in the
[implementation plan](docs/planning/phase-3-plan.md) (§17) is true. The pull request template
repeats it as a checklist; "N/A" needs a reason.

Non-negotiable in every change:

- Tenant context comes from the authenticated session. Never trust `organization_id`,
  `user_id`, `project_id` or `task_id` sent by a client; load and verify them.
- New tenant data has `organization_id`, composite foreign keys and a row-level security
  policy, and new routes, jobs, cache keys and files are covered by cross-tenant tests.
- Screenshots, activity, app and URL usage, location and employee data are sensitive; no
  endpoint exposes them without an authorization policy and a test proving the policy.
- Integrity rules live in the database (foreign keys, unique and check constraints) as well
  as in code.
- No secrets, tokens or environment-specific values in source.

## Branch protection

Apply these settings to `main` on GitHub (Settings → Branches) once CI lands in E00-S04:

- Require a pull request with one approving review, including a code owner.
- Dismiss stale approvals when new commits are pushed.
- Require status checks to pass and branches to be up to date before merging.
- Require linear history (squash merge only) and conversation resolution.
- Block force pushes and deletions.
