# Mobile app decisions

Decisions made while scaffolding the mobile app (E00-S09). The stack (React Native with Expo,
development builds) is fixed by the architecture's mobile section.

## Generated native projects

`ios/` and `android/` are generated from `app.config.ts` by `expo prebuild` (Expo's Continuous
Native Generation) and ignored by Git. Native configuration lives in `app.config.ts` and config
plugins, so SDK upgrades don't mean merging changes into Xcode and Gradle projects. A native change
that config plugins cannot express needs a local plugin in `apps/mobile/plugins/`, not a committed
native folder.

## Builds on CI runners, not EAS

CI builds the Android APK with Gradle and the iOS simulator app with `xcodebuild` on GitHub's
runners. Store builds may move to EAS Build in E24; this scaffold needs no Expo account or token.

Expo SDK 57 requires Xcode 26.4 or later, so the iOS job runs on `macos-26`, whose default Xcode
meets that. On `macos-15` the build failed while resolving Swift packages for ExpoModulesJSI. The
desktop workflow still uses `macos-15`, which Tauri supports; move it when that image retires.

## React versions

Expo SDK 57 requires React 19.2.3, while the web app uses React 19.3. pnpm keeps them apart per
package. The mobile package declares `react-dom@19.2.3` as a dev dependency so that Expo's optional
`react-dom` peer resolves to the matching version instead of the web app's. `test-renderer` is
pinned to 1.2.0 for the same reason (1.3 requires React 19.3).

## Scripts

The generator script is `native:generate`, not `prebuild`: pnpm runs a script named `prebuild`
automatically before `build`, which would regenerate the native projects on every bundle export.

## Permissions

`app.config.ts` requests no Android permissions and blocks `SYSTEM_ALERT_WINDOW`. Each permission
is added by the story that needs it (background location with E27), together with its privacy
review.

## Deferred

- Navigation, sign-in and the app shell: E24-S01.
- Localisation: with the first real screens in E24.
- Store signing, EAS Update and releases: E24.
