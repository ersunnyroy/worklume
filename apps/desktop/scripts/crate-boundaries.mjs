// @ts-check
/**
 * Dependency rules between the agent's crates (architecture §5). The core never talks to the
 * OS or the network directly, and nothing but the shell knows about Tauri or the webview.
 */

/** Workspace crates each crate may depend on. A new crate needs an entry here. */
export const allowedWorkspaceDependencies = {
  'worklume-platform': [],
  'worklume-storage': ['worklume-platform'],
  'worklume-core': ['worklume-platform', 'worklume-storage'],
  'worklume-sync': ['worklume-core', 'worklume-platform', 'worklume-storage'],
  'worklume-desktop': ['worklume-core', 'worklume-platform', 'worklume-storage', 'worklume-sync'],
};

/** External crates that only one layer may use directly. */
export const restrictedCrates = [
  {
    pattern: /^(tauri|tauri-.+|specta|specta-.+)$/,
    allowedIn: ['worklume-desktop'],
    reason: 'the webview, IPC and app lifecycle belong to the shell',
  },
  {
    pattern:
      /^(objc2(-.+)?|block2|core-foundation|core-graphics|windows|windows-sys|x11rb|xcb|zbus|ashpd|atspi|wayland-client)$/,
    allowedIn: ['worklume-platform'],
    reason: 'operating-system APIs belong in the platform adapters',
  },
  {
    pattern: /^(reqwest|hyper|hyper-util|ureq|isahc|surf)$/,
    allowedIn: ['worklume-sync'],
    reason: 'network access belongs to sync',
  },
];

/**
 * @typedef {{ name: string, kind: string | null, path?: string }} Dependency
 * @typedef {{ name: string, dependencies: Dependency[] }} Package
 */

/**
 * Checks direct normal and build dependencies (dev-dependencies are exempt, so tests can use
 * Tauri's mock runtime). Returns one message per violation.
 *
 * @param {{ packages: Package[] }} metadata output of `cargo metadata --no-deps`
 * @returns {string[]}
 */
export function checkBoundaries(metadata) {
  const allowed = /** @type {Record<string, string[] | undefined>} */ (
    allowedWorkspaceDependencies
  );
  const workspaceNames = new Set(metadata.packages.map((pkg) => pkg.name));
  const violations = [];

  for (const pkg of metadata.packages) {
    const allowedWorkspace = allowed[pkg.name];
    if (allowedWorkspace === undefined) {
      violations.push(`${pkg.name} has no entry in allowedWorkspaceDependencies`);
      continue;
    }
    for (const dependency of pkg.dependencies) {
      if (dependency.kind === 'dev') {
        continue;
      }
      if (workspaceNames.has(dependency.name)) {
        if (!allowedWorkspace.includes(dependency.name)) {
          violations.push(`${pkg.name} may not depend on ${dependency.name}`);
        }
        continue;
      }
      for (const rule of restrictedCrates) {
        if (rule.pattern.test(dependency.name) && !rule.allowedIn.includes(pkg.name)) {
          violations.push(`${pkg.name} may not depend on ${dependency.name}: ${rule.reason}`);
        }
      }
    }
  }
  return violations;
}
