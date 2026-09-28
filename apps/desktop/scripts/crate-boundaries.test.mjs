// @ts-check
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { checkBoundaries } from './crate-boundaries.mjs';

/** @param {string} name @param {[string, (string | null)?][]} dependencies */
function crate(name, dependencies = []) {
  return { name, dependencies: dependencies.map(([dep, kind = null]) => ({ name: dep, kind })) };
}

/** The workspace as it should be, plus the given replacements. @param {ReturnType<typeof crate>[]} overrides */
function workspace(...overrides) {
  const base = [
    crate('worklume-platform'),
    crate('worklume-storage', [['worklume-platform']]),
    crate('worklume-core', [['worklume-platform'], ['worklume-storage']]),
    crate('worklume-sync', [['worklume-core'], ['worklume-storage']]),
    crate('worklume-desktop', [['tauri'], ['specta'], ['worklume-core'], ['tauri-build', 'build']]),
  ];
  const byName = new Map(base.map((pkg) => [pkg.name, pkg]));
  for (const pkg of overrides) {
    byName.set(pkg.name, pkg);
  }
  return { packages: [...byName.values()] };
}

describe('checkBoundaries', () => {
  it('accepts the intended layering', () => {
    assert.deepEqual(checkBoundaries(workspace()), []);
  });

  it('keeps Tauri out of the core', () => {
    const violations = checkBoundaries(workspace(crate('worklume-core', [['tauri']])));
    assert.match(violations.join('\n'), /worklume-core may not depend on tauri/);
  });

  it('keeps OS APIs in the platform crate', () => {
    assert.deepEqual(
      checkBoundaries(workspace(crate('worklume-platform', [['objc2-app-kit'], ['windows']]))),
      [],
    );
    const violations = checkBoundaries(workspace(crate('worklume-core', [['windows']])));
    assert.match(violations.join('\n'), /operating-system APIs/);
  });

  it('keeps network access in sync', () => {
    const violations = checkBoundaries(workspace(crate('worklume-storage', [['reqwest']])));
    assert.match(violations.join('\n'), /network access belongs to sync/);
  });

  it('forbids upward workspace dependencies', () => {
    const violations = checkBoundaries(workspace(crate('worklume-core', [['worklume-sync']])));
    assert.deepEqual(violations, ['worklume-core may not depend on worklume-sync']);
  });

  it('exempts dev-dependencies', () => {
    assert.deepEqual(checkBoundaries(workspace(crate('worklume-core', [['tauri', 'dev']]))), []);
  });

  it('requires a rule for every new crate', () => {
    const violations = checkBoundaries(workspace(crate('worklume-ai')));
    assert.deepEqual(violations, ['worklume-ai has no entry in allowedWorkspaceDependencies']);
  });
});
