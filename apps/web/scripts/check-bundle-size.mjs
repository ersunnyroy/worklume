// @ts-check
/**
 * Fails the build when the code a browser downloads before first render grows past budget.
 * "Initial" means the entry script, its modulepreload chunks and the stylesheets that
 * index.html references; lazily loaded route chunks are not counted.
 */
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { gzipSync } from 'node:zlib';

const BUDGET_BYTES = { js: 160 * 1024, css: 20 * 1024 };
const dist = join(import.meta.dirname, '..', 'dist');

const html = await readFile(join(dist, 'index.html'), 'utf8');
const assets = [...html.matchAll(/(?:src|href)="\/(assets\/[^"]+\.(js|css))"/g)].map((match) => ({
  path: match[1] ?? '',
  kind: /** @type {'js' | 'css'} */ (match[2]),
}));

if (assets.length === 0) {
  throw new Error('No assets found in dist/index.html; did the build run?');
}

const totals = { js: 0, css: 0 };
for (const asset of assets) {
  const size = gzipSync(await readFile(join(dist, asset.path)), { level: 9 }).length;
  totals[asset.kind] += size;
  console.log(`${asset.path.padEnd(40)} ${(size / 1024).toFixed(1).padStart(7)} KB gzip`);
}

let failed = false;
for (const kind of /** @type {const} */ (['js', 'css'])) {
  const line = `Initial ${kind.toUpperCase()}: ${(totals[kind] / 1024).toFixed(1)} KB of ${String(BUDGET_BYTES[kind] / 1024)} KB`;
  if (totals[kind] > BUDGET_BYTES[kind]) {
    console.error(`${line} (over budget)`);
    failed = true;
  } else {
    console.log(line);
  }
}
process.exitCode = failed ? 1 : 0;
