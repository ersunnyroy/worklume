// @ts-check
// The desktop agent's events against the contract. `test/fixtures/desktop-events.json` is
// written by the agent's fixture test (apps/desktop/sync/tests/contract_fixture.rs) from a real
// tracker run; here every event must match the `DesktopEvent` schema, and every event type the
// schema lists must appear.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { before, describe, it } from 'node:test';

import { Ajv2020 } from 'ajv/dist/2020.js';
import ajvFormats from 'ajv-formats';
import { parse } from 'yaml';

const root = join(import.meta.dirname, '..');

/** @typedef {{ type: string, payload: Record<string, unknown> } & Record<string, unknown>} DesktopEvent */

/** @type {import('ajv').ValidateFunction} */
let validate;
/** @type {DesktopEvent[]} */
let events;
/** @type {string[]} */
let schemaTypes;

// ajv-formats is CommonJS: the default import is the module, whose `default` is the plugin.
const addFormats = ajvFormats.default;

before(async () => {
  const spec = parse(await readFile(join(root, 'openapi.yaml'), 'utf8'));
  // OpenAPI keywords such as `discriminator` and `example` are not JSON Schema; allow them.
  const ajv = new Ajv2020({ strict: false, allErrors: true });
  addFormats(ajv);
  ajv.addSchema(spec, 'openapi.json');
  const compiled = ajv.getSchema('openapi.json#/components/schemas/DesktopEvent');
  assert.ok(compiled, 'the contract defines DesktopEvent');
  validate = compiled;
  events = JSON.parse(await readFile(join(root, 'test/fixtures/desktop-events.json'), 'utf8'));
  schemaTypes = Object.keys(spec.components.schemas.DesktopEvent.discriminator.mapping);
});

/** @param {unknown} event */
function errors(event) {
  return validate(event) ? [] : (validate.errors ?? []).map((error) => ajvMessage(error));
}

/** @param {import('ajv').ErrorObject} error */
function ajvMessage(error) {
  return `${error.instancePath} ${error.message ?? ''}`.trim();
}

describe('desktop events', () => {
  it('match the DesktopEvent schema', () => {
    assert.ok(events.length > 0);
    for (const event of events) {
      assert.deepEqual(errors(event), [], `${event.type} (seq ${String(event['seq'])})`);
    }
  });

  it('cover every event type the contract defines, and no others', () => {
    assert.deepEqual(
      [...new Set(events.map((event) => event.type))].sort(),
      [...schemaTypes].sort(),
    );
  });

  it('are refused when a payload field is missing, extra or of the wrong kind', () => {
    const started = events.find((event) => event.type === 'timer.started');
    assert.ok(started);
    const { project_id: _dropped, ...withoutProject } = started.payload;
    for (const broken of [
      { ...started, payload: withoutProject },
      {
        ...started,
        payload: { ...started.payload, organization_id: started.payload['project_id'] },
      },
      { ...started, payload: { ...started.payload, trigger: 'someone' } },
      { ...started, type: 'timer.launched' },
      { ...started, wall_time: '21 September 2026' },
      { ...started, surprise: true },
    ]) {
      assert.notDeepEqual(errors(broken), [], JSON.stringify(broken));
    }
  });

  it('say who decided about idle time', () => {
    const resolved = events.find((event) => event.type === 'idle.resolved');
    assert.ok(resolved);
    assert.deepEqual(
      errors({ ...resolved, payload: { ...resolved.payload, decided_by: 'policy' } }),
      [],
    );
    const { decided_by: _dropped, ...undecided } = resolved.payload;
    for (const payload of [undecided, { ...resolved.payload, decided_by: 'server' }]) {
      assert.notDeepEqual(errors({ ...resolved, payload }), [], JSON.stringify(payload));
    }
  });

  it('refuse usage detail the agent never sends', () => {
    const apps = events.find((event) => event.type === 'app_usage.batch');
    const urls = events.find((event) => event.type === 'url_usage.batch');
    assert.ok(apps && urls);
    const app = /** @type {Record<string, unknown>[]} */ (apps.payload['apps'])[0];
    const site = /** @type {Record<string, unknown>[]} */ (urls.payload['sites'])[0];
    for (const broken of [
      { ...urls, payload: { ...urls.payload, sites: [{ ...site, path: '/search?q=salary' }] } },
      { ...urls, payload: { ...urls.payload, sites: [{ ...site, path: '/a#b' }] } },
      { ...urls, payload: { ...urls.payload, sites: [{ ...site, url: 'https://github.com/' }] } },
      { ...urls, payload: { ...urls.payload, sites: [] } },
      { ...apps, payload: { ...apps.payload, apps: [{ ...app, title: '' }] } },
      { ...apps, payload: { ...apps.payload, apps: [{ ...app, seconds: 0 }] } },
      { ...apps, payload: { ...apps.payload, apps: [{ ...app, seconds: 601 }] } },
      { ...apps, payload: { ...apps.payload, member_id: apps.payload['slot_id'] } },
    ]) {
      assert.notDeepEqual(errors(broken), [], JSON.stringify(broken.payload));
    }
    assert.deepEqual(
      errors({ ...urls, payload: { ...urls.payload, sites: [{ ...site, path: null }] } }),
      [],
    );
  });

  it('carry input features as statistics, never event times', () => {
    const slot = events.find(
      (event) => event.type === 'slot.closed' && event.payload['input_features'] !== null,
    );
    assert.ok(slot);
    /** @typedef {Record<string, unknown>} Intervals */
    const features =
      /** @type {{ events: Record<string, number>, intervals: Record<string, Intervals> } & Record<string, unknown>} */ (
        slot.payload['input_features']
      );
    const { intervals } = features;
    /** @param {unknown} inputFeatures */
    const withFeatures = (inputFeatures) => ({
      ...slot,
      payload: { ...slot.payload, input_features: inputFeatures },
    });
    assert.deepEqual(errors(withFeatures(null)), []);
    assert.deepEqual(
      errors(withFeatures({ ...features, intervals: { ...intervals, key: null } })),
      [],
    );
    assert.deepEqual(
      errors(
        withFeatures({
          ...features,
          intervals: { ...intervals, key: { count: 3, entropy_bits: null, cv: null } },
        }),
      ),
      [],
    );
    for (const broken of [
      { ...features, event_times: [1_789_999_380_137] },
      { ...features, version: 2 },
      { ...features, keyboard_share: 1.5 },
      { ...features, events: { ...features.events, key: -1 } },
      { ...features, events: { ...features.events, keys_pressed: ['a'] } },
      { ...features, intervals: { ...intervals, double_click: null } },
      { ...features, intervals: { ...intervals, key: { ...intervals['key'], entropy_bits: 4.5 } } },
      { ...features, intervals: { ...intervals, key: { ...intervals['key'], count: 0 } } },
      { ...features, intervals: { ...intervals, key: { ...intervals['key'], buckets: [1, 2] } } },
      { ...features, intervals: { ...intervals, any: { ...intervals['any'], cv: -0.1 } } },
    ]) {
      assert.notDeepEqual(errors(withFeatures(broken)), [], JSON.stringify(broken));
    }
    const { input_features: _dropped, ...withoutFeatures } = slot.payload;
    assert.notDeepEqual(errors({ ...slot, payload: withoutFeatures }), []);
  });

  it('keep slots within their bounds', () => {
    const slot = events.find((event) => event.type === 'slot.closed');
    assert.ok(slot);
    for (const payload of [
      { ...slot.payload, tracked_seconds: 601 },
      { ...slot.payload, tracked_seconds: 0 },
      { ...slot.payload, flags: ['partial_slot', 'partial_slot'] },
      { ...slot.payload, input_method: 'telepathy' },
    ]) {
      assert.notDeepEqual(errors({ ...slot, payload }), [], JSON.stringify(payload));
    }
  });
});
