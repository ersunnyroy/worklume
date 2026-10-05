// @ts-check
// The desktop agent's capability reports against the contract.
// `test/fixtures/desktop-capabilities.json` is written by the agent's fixture test
// (apps/desktop/sync/tests/capabilities_fixture.rs): sample reports encoded as the agent sends
// them, and every status and reason code the agent knows. Here every report must match the
// `DesktopCapabilities` schema, and the agent's codes must be exactly the contract's.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { before, describe, it } from 'node:test';

import { Ajv2020 } from 'ajv/dist/2020.js';
import { parse } from 'yaml';

const root = join(import.meta.dirname, '..');

/** @typedef {{ status: string, reasons: string[] }} Capability */
/** @typedef {{ platform: string, desktop?: string, features: Record<string, Capability> } & Record<string, unknown>} Report */
/** @typedef {{ statuses: string[], reasons: string[], reports: { name: string, body: Report }[] }} Fixture */

/** @type {import('ajv').ValidateFunction} */
let validate;
/** @type {Fixture} */
let fixture;
/** @type {Record<string, { enum: string[] }>} */
let schemas;

before(async () => {
  const spec = parse(await readFile(join(root, 'openapi.yaml'), 'utf8'));
  const ajv = new Ajv2020({ strict: false, allErrors: true });
  ajv.addSchema(spec, 'openapi.json');
  const compiled = ajv.getSchema('openapi.json#/components/schemas/DesktopCapabilities');
  assert.ok(compiled, 'the contract defines DesktopCapabilities');
  validate = compiled;
  schemas = spec.components.schemas;
  fixture = JSON.parse(
    await readFile(join(root, 'test/fixtures/desktop-capabilities.json'), 'utf8'),
  );
});

/** @param {unknown} report */
function errors(report) {
  return validate(report)
    ? []
    : (validate.errors ?? []).map((error) => `${error.instancePath} ${error.message ?? ''}`);
}

/** @param {string} name */
function sample(name) {
  const found = fixture.reports.find((report) => report.name === name);
  assert.ok(found, `the fixture has the ${name} report`);
  return found.body;
}

/**
 * @param {Report} report
 * @param {string} feature
 * @param {Record<string, unknown>} capability a capability, or a malformed one
 */
function withFeature(report, feature, capability) {
  return { ...report, features: { ...report.features, [feature]: capability } };
}

describe('desktop capabilities', () => {
  it('match the DesktopCapabilities schema', () => {
    assert.ok(fixture.reports.length > 0);
    for (const { name, body } of fixture.reports) {
      assert.deepEqual(errors(body), [], name);
    }
  });

  it('use exactly the statuses and reasons the contract lists', () => {
    assert.deepEqual(
      [...fixture.statuses].sort(),
      [...(schemas['DesktopCapabilityStatus']?.enum ?? [])].sort(),
    );
    assert.deepEqual(
      [...fixture.reasons].sort(),
      [...(schemas['DesktopCapabilityReason']?.enum ?? [])].sort(),
    );
  });

  it('show a GNOME Wayland device as reduced, with reasons', () => {
    const gnome = sample('gnome_wayland_first_login');
    assert.equal(gnome.platform, 'linux_wayland');
    assert.equal(gnome.desktop, 'gnome');
    assert.ok(Object.values(gnome.features).some((feature) => feature.status !== 'granted'));
  });

  it('explain every status but granted, and never explain granted', () => {
    const x11 = sample('ubuntu_x11');
    assert.deepEqual(errors(withFeature(x11, 'idle', { status: 'granted', reasons: [] })), []);
    for (const broken of [
      withFeature(x11, 'idle', { status: 'granted', reasons: ['adapter_failed'] }),
      withFeature(x11, 'idle', { status: 'reduced', reasons: [] }),
      withFeature(x11, 'idle', { status: 'failed', reasons: [] }),
    ]) {
      assert.notDeepEqual(errors(broken), [], JSON.stringify(broken.features['idle']));
    }
  });

  it('refuse IDs, personal data and unknown codes', () => {
    const gnome = sample('gnome_wayland_first_login');
    const { session_lock: _dropped, ...withoutLock } = gnome.features;
    for (const broken of [
      { ...gnome, organization_id: '0192f0a4-7c1e-7d3a-9b2f-5d8e6a4c3b21' },
      { ...gnome, device_id: '0192f0a4-7c1e-7d3a-9b2f-5d8e6a4c3b21' },
      { ...gnome, features: withoutLock },
      { ...gnome, features: { ...gnome.features, keystrokes: { status: 'granted', reasons: [] } } },
      withFeature(gnome, 'apps', { status: 'granted', reasons: [], app: 'Firefox' }),
      withFeature(gnome, 'apps', { status: 'reduced', reasons: ['window_title_hidden'] }),
      withFeature(gnome, 'apps', { status: 'partial', reasons: ['adapter_failed'] }),
      withFeature(gnome, 'apps', {
        status: 'failed',
        reasons: ['adapter_failed', 'adapter_failed'],
      }),
      { ...gnome, platform: 'linux' },
      { ...gnome, desktop: 'ubuntu:GNOME' },
    ]) {
      assert.notDeepEqual(errors(broken), [], JSON.stringify(broken));
    }
  });

  it('leave the desktop out where there is none', () => {
    const mac = sample('macos_today');
    assert.equal(mac.desktop, undefined);
    assert.deepEqual(errors(mac), []);
  });
});
