// @ts-check
import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, before, describe, it } from 'node:test';
import { promisify } from 'node:util';

const run = promisify(execFile);
const root = join(import.meta.dirname, '..');
const problem = { $ref: '#/components/responses/Problem' };

/** A small contract that follows every Worklume rule; each case below breaks exactly one. */
function validContract() {
  return {
    openapi: '3.1.1',
    info: {
      title: 'Fixture',
      version: '1.0.0',
      description: 'Rule fixture.',
      contact: { name: 'Worklume engineering' },
    },
    servers: [{ url: 'https://api.worklume.com' }],
    tags: [{ name: 'Fixture', description: 'Fixture operations.' }],
    security: [{ bearerAuth: [] }],
    paths: {
      '/health': {
        'x-worklume-unversioned': true,
        get: {
          operationId: 'getHealth',
          tags: ['Fixture'],
          description: 'Liveness.',
          security: [],
          responses: { 200: { description: 'OK.' }, default: problem },
        },
      },
      '/v1/time-entries': {
        post: {
          operationId: 'createTimeEntry',
          tags: ['Fixture'],
          description: 'Create a time entry.',
          'x-worklume-idempotency': 'key-required',
          parameters: [{ $ref: '#/components/parameters/IdempotencyKey' }],
          requestBody: {
            content: {
              'application/json': {
                schema: {
                  type: 'object',
                  properties: {
                    project_id: { type: 'string' },
                    period: {
                      type: 'object',
                      properties: { started_at: { type: 'string' } },
                    },
                  },
                },
              },
            },
          },
          responses: { 201: { description: 'Created.' }, 422: problem, default: problem },
        },
      },
      '/v1/time-entries/{time_entry_id}': {
        parameters: [
          { name: 'time_entry_id', in: 'path', required: true, schema: { type: 'string' } },
        ],
        get: {
          operationId: 'getTimeEntry',
          tags: ['Fixture'],
          description: 'One time entry.',
          responses: { 200: { description: 'OK.' }, 404: problem, default: problem },
        },
      },
      '/v1/auth/switch-organization': {
        post: {
          operationId: 'switchOrganization',
          tags: ['Fixture'],
          description: 'Identity-level: mints a token for another organization.',
          'x-worklume-idempotency': 'natural',
          'x-worklume-allows-organization-id': true,
          requestBody: {
            content: {
              'application/json': {
                schema: { type: 'object', properties: { organization_id: { type: 'string' } } },
              },
            },
          },
          responses: { 200: { description: 'Tokens.' }, default: problem },
        },
      },
    },
    components: {
      securitySchemes: { bearerAuth: { type: 'http', scheme: 'bearer' } },
      parameters: {
        IdempotencyKey: {
          name: 'Idempotency-Key',
          in: 'header',
          required: true,
          schema: { type: 'string' },
        },
      },
      responses: {
        Problem: {
          description: 'Problem details.',
          content: {
            'application/problem+json': {
              schema: { type: 'object', properties: { request_id: { type: 'string' } } },
            },
          },
        },
      },
    },
  };
}

/** @typedef {ReturnType<typeof validContract>} Contract */
/** @type {Record<string, { mutate: (contract: any) => void, expect: string[] }>} */
const cases = {
  valid: { mutate: () => undefined, expect: [] },
  'organization ID as a query parameter': {
    mutate: (c) => {
      c.paths['/v1/time-entries/{time_entry_id}'].get.parameters = [
        { name: 'organization_id', in: 'query', schema: { type: 'string' } },
      ];
    },
    expect: ['worklume-no-client-organization-id-parameter'],
  },
  'organization ID as a path-level parameter': {
    mutate: (c) => {
      c.paths['/v1/time-entries/{time_entry_id}'].parameters.push({
        name: 'org_id',
        in: 'header',
        schema: { type: 'string' },
      });
    },
    expect: ['worklume-no-client-organization-id-parameter'],
  },
  'organization ID nested in a request body': {
    mutate: (c) => {
      c.paths['/v1/time-entries'].post.requestBody.content[
        'application/json'
      ].schema.properties.period.properties.organization_id = { type: 'string' };
    },
    expect: ['worklume-no-client-organization-id-body'],
  },
  'camelCase property': {
    mutate: (c) => {
      c.paths['/v1/time-entries'].post.requestBody.content[
        'application/json'
      ].schema.properties.projectId = { type: 'string' };
    },
    expect: ['worklume-snake-case-properties'],
  },
  'error response as plain JSON': {
    mutate: (c) => {
      c.paths['/v1/time-entries'].post.responses[422] = {
        description: 'Invalid.',
        content: { 'application/json': { schema: { type: 'object' } } },
      };
    },
    expect: ['worklume-problem-json-errors'],
  },
  'unversioned path': {
    mutate: (c) => {
      c.paths['/time-entries'] = c.paths['/v1/time-entries'];
      delete c.paths['/v1/time-entries'];
    },
    expect: ['worklume-versioned-paths'],
  },
  'camelCase path segment': {
    mutate: (c) => {
      c.paths['/v1/timeEntries'] = c.paths['/v1/time-entries'];
      delete c.paths['/v1/time-entries'];
    },
    expect: ['worklume-path-casing'],
  },
  'snake_case operation ID': {
    mutate: (c) => {
      c.paths['/v1/time-entries/{time_entry_id}'].get.operationId = 'get_time_entry';
    },
    expect: ['worklume-operation-id-camel-case'],
  },
  'POST without an idempotency mode': {
    mutate: (c) => {
      delete c.paths['/v1/time-entries'].post['x-worklume-idempotency'];
    },
    expect: ['worklume-idempotency-declared'],
  },
  'key-required POST without the header': {
    mutate: (c) => {
      delete c.paths['/v1/time-entries'].post.parameters;
      delete c.components.parameters;
    },
    expect: ['worklume-idempotency-declared'],
  },
  'key-required POST with an optional header': {
    mutate: (c) => {
      c.components.parameters.IdempotencyKey.required = false;
    },
    expect: ['worklume-idempotency-declared'],
  },
  'organization ID in a body without the opt-out': {
    mutate: (c) => {
      delete c.paths['/v1/auth/switch-organization'].post['x-worklume-allows-organization-id'];
    },
    expect: ['worklume-no-client-organization-id-body'],
  },
};

/** @type {Map<string, { code: string, severity: number }[]>} */
const resultsByCase = new Map();
/** @type {string} */
let directory;

before(async () => {
  directory = await mkdtemp(join(tmpdir(), 'worklume-contract-'));
  /** @type {[name: string, file: string][]} */
  const files = [];
  for (const [name, { mutate }] of Object.entries(cases)) {
    const contract = validContract();
    mutate(contract);
    const file = join(directory, `${name.replaceAll(/\W+/g, '-')}.json`);
    await writeFile(file, JSON.stringify(contract));
    files.push([name, file]);
  }

  // One Spectral run for every fixture; it exits 1 when it reports errors, which is expected.
  const args = [
    'exec',
    'spectral',
    'lint',
    '--ruleset',
    '.spectral.yaml',
    '--format',
    'json',
    '--quiet',
  ];
  const output = await run('pnpm', [...args, ...files.map(([, file]) => file)], {
    cwd: root,
    maxBuffer: 10 * 1024 * 1024,
  }).catch((/** @type {{ stdout?: string }} */ error) => ({ stdout: error.stdout ?? '' }));
  /** @type {{ code: string, severity: number, source: string }[]} */
  const results = JSON.parse(output.stdout || '[]');
  for (const [name, file] of files) {
    resultsByCase.set(
      name,
      results.filter((result) => result.source === file),
    );
  }
});

after(async () => {
  await rm(directory, { recursive: true, force: true });
});

describe('Spectral rules', () => {
  it('report nothing for a contract that follows the conventions', () => {
    assert.deepEqual(resultsByCase.get('valid'), []);
  });

  for (const [name, { expect }] of Object.entries(cases)) {
    if (name === 'valid') {
      continue;
    }
    it(`report ${expect.join(', ')} for: ${name}`, () => {
      const codes = (resultsByCase.get(name) ?? []).map((result) => result.code);
      assert.deepEqual([...new Set(codes)].sort(), [...expect].sort());
    });
  }
});
