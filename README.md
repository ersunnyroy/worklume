# Worklume

Time tracking, activity insight and workforce management for distributed teams. This
monorepo holds the API, the web app, the desktop agent, the mobile app and the code they
share.

## Layout

| Path                     | What lives there                                                                | Arrives in |
| ------------------------ | ------------------------------------------------------------------------------- | ---------- |
| `apps/api`               | Laravel API, modular monolith (`app/Domains/*`)                                 | E00-S02    |
| `apps/web`               | React web app (Vite, TanStack Router and Query); see its README                 | E00-S08    |
| `apps/desktop`           | Tauri 2 desktop agent: Rust core with per-OS adapters, React UI; see its README | E00-S09    |
| `apps/mobile`            | Expo (React Native) app                                                         | E00-S09    |
| `packages/tsconfig`      | Shared TypeScript settings                                                      | E00-S01    |
| `packages/eslint-config` | Shared ESLint flat configs (`base`, `react`)                                    | E00-S01    |
| `docs/planning`          | Research, architecture and implementation plan (source of truth)                | —          |
| `docs/conventions`       | Repository conventions and tooling decisions                                    | —          |

## Prerequisites

| Tool       | Version             | Needed for            |
| ---------- | ------------------- | --------------------- |
| Node.js    | 22.13 or later      | Everything TypeScript |
| pnpm       | 12.6 (via Corepack) | Workspace installs    |
| Rust       | 1.95.0 (pinned)     | Desktop agent         |
| PHP        | 8.4                 | API (from E00-S02)    |
| Composer   | 2                   | API (from E00-S02)    |
| PostgreSQL | 17                  | API (from E00-S06)    |
| Valkey     | 8                   | API (from E00-S06)    |

`rust-toolchain.toml` makes `rustup` install the pinned Rust version on first use. The desktop
app also needs the platform's webview libraries; see [apps/desktop/README.md](apps/desktop/README.md).

## Getting started

```sh
corepack enable
pnpm install
pnpm check
```

Use Corepack (or pnpm's standalone installer) rather than an older global pnpm; see
[docs/conventions/tooling.md](docs/conventions/tooling.md) for why.

`pnpm check` runs everything CI runs for TypeScript: formatting, lint, typecheck, tests and
build, across every workspace package, with Turborepo caching.

## Common commands

| Command          | Does                                          |
| ---------------- | --------------------------------------------- |
| `pnpm check`     | Format check, lint, typecheck, test and build |
| `pnpm dev`       | Start every app's dev server                  |
| `pnpm test:e2e`  | Browser smoke tests against production builds |
| `pnpm lint`      | Lint every package                            |
| `pnpm typecheck` | Typecheck every package                       |
| `pnpm test`      | Run every package's tests                     |
| `pnpm build`     | Build every package                           |
| `pnpm format`    | Rewrite files with Prettier                   |

Run one package's task with a filter, for example `pnpm --filter @worklume/eslint-config test`.

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) for branches, commits and the Definition of Done,
and [docs/README.md](docs/README.md) for where decisions are recorded.
