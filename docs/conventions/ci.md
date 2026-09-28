# Continuous integration

Decisions for the pull request checks (E00-S04, first part). The API image build and the deploy
to dev join this pipeline once the API (E00-S02) and the infrastructure (E00-S03) exist.

## Workflows

| Workflow      | Runs when                                             | What it does                                                                                    |
| ------------- | ----------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| `ci.yml`      | Every pull request, pushes to `main`, weekly, by hand | Commit conventions, format, lint, typecheck, tests, builds, browser smoke tests, security scans |
| `desktop.yml` | Desktop paths change, or by hand                      | Lint and tests on macOS, Windows and Ubuntu; unsigned installers                                |
| `mobile.yml`  | Mobile paths change, or by hand                       | Checks, a release APK and an unsigned iOS simulator app                                         |

`ci.yml` runs on every pull request, so its jobs can be required status checks. Path-filtered
workflows cannot be required: a check that never starts would block the merge forever.

## Jobs in `ci.yml`

- **Commit conventions** (pull requests only): commitlint on the pull request title, which
  becomes the squash commit on `main`, and on every commit in the pull request. The title is
  passed through an environment variable, never interpolated into the script, so a crafted title
  cannot inject shell commands.
- **Lint, typecheck, test and build**: `pnpm format:check`, then Turborepo runs lint, typecheck,
  test and build for every package except the desktop app, which needs Rust and WebKitGTK and is
  covered by `desktop.yml`. The job then fails if the build changed any tracked file (for example
  a stale `apps/web/src/routeTree.gen.ts`) and runs the web app's Playwright smoke tests.
- **Security scans** (also weekly, so new advisories surface without a code change):
  - gitleaks over the full history, from a release pinned by version and SHA-256.
  - `pnpm audit --audit-level high`: high and critical advisories fail the build. Moderate ones
    are reviewed by hand. At the time of writing there is one, in `uuid` via Expo's build tooling,
    which affects an API the tooling does not use.
  - `cargo audit`: vulnerabilities fail the build. RustSec warnings do not; the current ones
    (`proc-macro-error` unmaintained, `glib` 0.18 unsound) come from the GTK 3 bindings Tauri uses
    on Linux and go away when Tauri moves off them.
  - Semgrep with the `p/default`, `p/github-actions` and `p/secrets` rule packs. `.semgrepignore`
    replaces Semgrep's default ignore list, so it lists dependency and build folders as well as
    generated code.

## Supply-chain rules for workflows

- Every third-party action is pinned to a full commit SHA, with the version in a comment.
- Workflows get a read-only `GITHUB_TOKEN` (`permissions: contents: read`) and check out code
  with `persist-credentials: false`.
- Downloaded tools are pinned by version and, where the project publishes checksums, verified.
- Superseded runs of the same branch are cancelled.

## Reading failures

Failures appear as annotations on the pull request's Checks tab. Steps whose tools print long
logs (for example `xcodebuild`) extract the error lines into an annotation, so the cause is
readable without downloading the raw log.

## Still to come in E00-S04

- API: Pint, Larastan, Pest and the migration checks against an ephemeral PostgreSQL (after
  E00-S02).
- Spectral lint of the OpenAPI contract (E00-S07).
- Terraform `fmt` and `validate` (E00-S03).
- Container image build and deploy to dev on merge (after E00-S02 and E00-S03).
- A 15-minute budget check on the whole pipeline once those jobs exist.
