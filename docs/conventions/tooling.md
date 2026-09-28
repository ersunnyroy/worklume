# Tooling decisions

Decisions made while setting up the monorepo (E00-S01). Each one records why, so it can be
revisited when its reason no longer holds.

## pnpm 12 and Turborepo 2

- pnpm workspaces cover `apps/*` and `packages/*`. Turborepo runs `lint`, `typecheck`, `test`
  and `build` in dependency order and caches the results.
- pnpm settings live in `pnpm-workspace.yaml`; since pnpm 11, `.npmrc` holds only registry and
  auth settings. `engineStrict` fails installs on unsupported Node or pnpm versions, and
  `saveExact` records exact versions so upgrades arrive as reviewed pull requests.
- `allowBuilds` is empty: no dependency may run install scripts until someone adds it here on
  purpose.
- Install pnpm through Corepack (`corepack enable`) or pnpm's own installer. Do not let an
  older global pnpm switch itself to 12 through `packageManager`: that path skips pnpm 12's
  native binary and leaves a launcher script that Turborepo cannot spawn ("Exec format error").
- `minimumReleaseAge: 1440` refuses any version published less than a day ago. Because it is set
  explicitly, pnpm fails on a too-new version instead of quietly adding it to an exclusion list.
  This is why Turborepo is pinned to 2.11.4: 2.11.5 was hours old when the repository was set up.
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
