# Tooling decisions

Decisions made while setting up the monorepo (E00-S01). Each one records why, so it can be
revisited when its reason no longer holds.

## pnpm 12 and Turborepo 2

- pnpm workspaces cover `apps/*` and `packages/*`. Turborepo runs `lint`, `typecheck`, `test`
  and `build` in dependency order and caches the results.
- pnpm settings live in `pnpm-workspace.yaml`; since pnpm 11, `.npmrc` holds only registry and
  auth settings. `engineStrict` fails installs on unsupported Node or pnpm versions, and
  `saveExact` records exact versions so upgrades arrive as reviewed pull requests.
- No dependency may run install scripts unless `allowBuilds` lists it as `true`. Entries set to
  `false` record a deliberate refusal (currently `@scarf/scarf`, which only sends install
  analytics).
- Install pnpm through Corepack (`corepack enable`) or pnpm's own installer. Do not let an
  older global pnpm switch itself to 12 through `packageManager`: that path skips pnpm 12's
  native binary and leaves a launcher script that Turborepo cannot spawn ("Exec format error").
- Supply-chain settings (tightened in E00-S04 after the Semgrep supply-chain rules flagged them):
  - `minimumReleaseAge: 10080` refuses any version published less than seven days ago. Hijacked
    releases are usually found and pulled within days. Because the setting is explicit,
    pnpm fails on a too-new version instead of quietly adding it to an exclusion list. The cost is
    that upgrades trail releases by a week; an urgent security fix can be let through with
    `minimumReleaseAgeExclude` in a reviewed change. Every pinned version was chosen to respect
    the window, which is why, for example, Turborepo is on 2.11.2.
  - `trustPolicy: no-downgrade` refuses a version published with weaker trust evidence than
    earlier releases (for example without provenance after provenance-signed ones), a common
    sign of a hijacked account. `trustPolicyIgnoreAfter: 43200` limits the check to versions from
    the last 30 days: many long-lived versions, such as `semver@6` and `undici-types@6`, predate
    their projects' move to provenance and would otherwise fail.
  - `blockExoticSubdeps: true` keeps transitive dependencies on the registry (no Git or tarball
    URLs). It is pnpm's default, stated explicitly so a change is visible in review.
  - Rust crates have no equivalent release-age setting in Cargo. They are pinned by `Cargo.lock`,
    checked by `cargo audit` and upgraded through reviewed pull requests.
- Turborepo 2.11.5 and later write an `AGENTS.md` block when they detect an AI agent. When
  upgrading, set `"agentGuidance": false` in `turbo.json`; agent instructions for this
  repository live in `CLAUDE.md`.

## TypeScript 6.0, not 7

TypeScript 7 is the current release, but `typescript-eslint` 8.x supports TypeScript below 6.1
only. Type-aware lint rules matter more than the newer compiler, so TypeScript is pinned to
6.0.3 until `typescript-eslint` supports 7.

## Shared TypeScript settings

`@worklume/tsconfig` provides `base.json`, `react-app.json` (DOM, JSX) and `node-library.json`
(NodeNext). The base turns on the strict flags the codebase relies on, including
`noUncheckedIndexedAccess` and `exactOptionalPropertyTypes`. The package typechecks a file of
`@ts-expect-error` cases, so relaxing any of these flags fails CI.

Packages using `node-library.json` must be ES modules (`"type": "module"`).

## Shared ESLint configs

`@worklume/eslint-config` exports `base()` and, from `/react`, `react()`. Both take
`tsconfigRootDir` (normally `import.meta.dirname`) and use `strictTypeChecked` plus
`stylisticTypeChecked`. Prettier owns formatting. Lint runs with `--max-warnings=0`.

The package's tests lint fixtures and assert the exact rules reported, so a config change that
silently disables a rule (for example `no-floating-promises`) fails the build.

## Rust workspace

The root `Cargo.toml` defines the shared edition, lints, release profile and pinned toolchain
(1.95.0). Its first member, the desktop shell, arrived in E00-S09. Rust tasks join Turborepo
through the desktop app's `package.json` scripts rather than root-level cargo calls; see
[desktop.md](desktop.md).

`unsafe_code` is denied workspace-wide. The per-OS platform adapters are the only crates
expected to opt in, and `clippy::undocumented_unsafe_blocks` requires a `SAFETY:` comment on
every block.

## Composer

PHP dependencies are managed per app (`apps/api/composer.json`) and arrive with E00-S02. The
API joins Turborepo through a thin `package.json` whose scripts call Composer, Pest, Larastan
and Pint.
