# Web app decisions

Decisions made while building the web app shell (E00-S08). The stack itself (Vite SPA,
TanStack Router and Query, Tailwind, Radix) is fixed by ADR-013 in the architecture.

## Runtime configuration instead of build-time variables

The release plan promotes the exact staging artifacts to production without rebuilding. Vite's
`import.meta.env` values are baked in at build time, which would force a build per environment.
The app instead fetches `/config.json` at startup and validates it; each deployment writes its
own copy. The file is public by nature, so it holds only the environment name and API URL.

## File-based routing with a committed route tree

TanStack Router's Vite plugin generates `src/routeTree.gen.ts` and splits every route into its own
chunk. The generated file is committed so `tsc` and ESLint work on a fresh clone without running
the generator first. It is excluded from Prettier and ESLint and marked as generated for GitHub.
CI (E00-S04) should fail if a build changes it.

## zod/mini for browser validation

The classic Zod API added about 14 KB gzipped to the startup bundle (152 KB against 138 KB for the
whole shell). `zod/mini` has the same validation with a functional API and tree-shakes to a few
kilobytes. ESLint blocks `import … from 'zod'` in `src/`. Shared schemas generated from OpenAPI in
E00-S07 should target `zod/mini` for the same reason.

## Typed translation keys

`src/i18n/i18next.d.ts` types `t()` against the English source, so a missing or misspelt key fails
the typecheck. English is the only language shipped; `negotiateLanguage` falls back to it, and adding
a language means adding a locale file and one entry in `resources`.

## Accessibility checks

- Vitest runs axe-core on rendered components in jsdom. jsdom has no layout, so colour contrast is
  checked only in the Playwright suite, in both light and dark schemes, against WCAG 2.2 AA rules.
- `eslint-plugin-jsx-a11y` does not support ESLint 10 yet, so static JSX accessibility linting is
  missing. Add it once it declares ESLint 10 support.

## Security headers and CSP

`apps/web/security-headers.json` is the single source for the app's response headers. CloudFront
applies it in production (E00-S03), and the preview server applies it so the E2E suite runs under
the real policy and fails on any CSP violation.

- `script-src 'self'`: no inline or third-party scripts. The Vite build emits none.
- `style-src 'self' 'unsafe-inline'`: required because Radix's scroll lock injects a `<style>`
  element with computed values, so neither a hash nor a nonce (impossible with static hosting)
  works. Inline styles cannot execute code; the residual risk is CSS-based content spoofing, which
  needs an HTML-injection bug first.
- `connect-src 'self' {{API_ORIGIN}}`: the placeholder is replaced per environment.
- `frame-ancestors 'none'` and `X-Frame-Options: DENY` prevent clickjacking.

## E2E server

Playwright starts `vite preview` directly, not through `pnpm preview`. Through the package-manager
wrapper the preview process survived Playwright's shutdown signal and the run never exited.

## Components ahead of their pages

`src/components/devices` (E13-S02) holds the device capability badge and explanation before the
devices page exists (E07-S07 needs the API). They are typed from the contract through
`@worklume/api-client`, a type-only import that adds nothing to the bundle, and their tests read
the desktop agent's fixture (`packages/api-contract/test/fixtures/desktop-capabilities.json`), so
a reason code the agent starts sending without words here fails the build. The web app's Turbo
`test` and `typecheck` inputs include that fixture.
