# @worklume/web

The signed-in Worklume web app: a Vite single-page app served as static files from S3 through
CloudFront. It talks to the API only; it holds no secrets and renders nothing on a server.

## Commands

| Command                                | Does                                                      |
| -------------------------------------- | --------------------------------------------------------- |
| `pnpm --filter @worklume/web dev`      | Dev server on http://localhost:5173 with hot reload       |
| `pnpm --filter @worklume/web build`    | Production build in `dist/`, then the bundle budget check |
| `pnpm --filter @worklume/web test`     | Vitest unit and component tests, with axe checks          |
| `pnpm --filter @worklume/web test:e2e` | Playwright smoke tests against the production build       |

The first E2E run needs a browser: `pnpm --filter @worklume/web exec playwright install chromium`.
Where Chromium is already installed, point `PLAYWRIGHT_CHROMIUM_EXECUTABLE` at it instead.

## Layout

| Path                     | Holds                                                                     |
| ------------------------ | ------------------------------------------------------------------------- |
| `src/main.tsx`           | Startup: load `config.json`, set up translations, render                  |
| `src/app/`               | App providers, router and query client                                    |
| `src/routes/`            | One file per route (TanStack Router file-based routing)                   |
| `src/routeTree.gen.ts`   | Generated from `src/routes` by the router plugin; committed, never edited |
| `src/components/layout/` | Shell: header, navigation, mobile drawer, page titles                     |
| `src/components/errors/` | Startup, not-found and route error screens                                |
| `src/config/`            | Runtime configuration and its React context                               |
| `src/i18n/`              | i18next setup and the English source strings                              |
| `e2e/`                   | Playwright smoke tests                                                    |

## Adding a page

1. Create `src/routes/<path>.tsx` exporting `Route = createFileRoute('/<path>')({ component })`.
   Run `pnpm dev` or `pnpm build` once so the plugin regenerates `routeTree.gen.ts`, and commit
   both files.
2. Put every user-facing string in `src/i18n/locales/en.json`; `t('missing.key')` is a type
   error.
3. Call `usePageTitle(...)` so the browser tab names the page.
4. Add the page to `src/components/layout/navigation.ts` if it belongs in the primary navigation.
5. Test it with Testing Library and `accessibilityViolations()`.

Each route is split into its own chunk automatically, so pages don't grow the startup bundle.

## Runtime configuration

The app reads `/config.json` before it renders, so the same build moves from staging to
production unchanged. Each deployment writes its own file:

```json
{ "environment": "production", "apiBaseUrl": "https://api.worklume.com" }
```

`environment` is `local`, `development`, `staging` or `production`; everything except
production shows a badge in the header. `apiBaseUrl` must use `https` outside `local`. Unknown
keys are rejected. The file is public, so it never holds secrets. If it is missing or invalid,
the app shows a startup error instead of a blank page.

## Deploying to S3 and CloudFront

Infrastructure arrives in E00-S03 and the pipeline in E00-S04. They must do the following:

- Upload `dist/assets/*` with `Cache-Control: public, max-age=31536000, immutable`; file names
  carry content hashes.
- Upload `index.html` and the environment's `config.json` with `Cache-Control: no-cache`.
- Serve `index.html` for any path that is not a file, so deep links such as `/reports/weekly`
  load the app. The router then shows the page or "Page not found".
- Send the headers in `security-headers.json` through a CloudFront response headers policy,
  replacing `{{API_ORIGIN}}` with the environment's API origin, and add HSTS.

The preview server (`pnpm preview`) sends the same headers and falls back to `index.html` in the
same way, so the E2E tests exercise the production behaviour.

## Budgets

The build fails if the JavaScript and CSS needed before first render exceed 160 KB and 20 KB
gzipped (`scripts/check-bundle-size.mjs`). The shell currently uses about 133 KB and 3 KB.
