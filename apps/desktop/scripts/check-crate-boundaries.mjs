// @ts-check
/** Fails when a crate in the Cargo workspace breaks the rules in crate-boundaries.mjs. */
import { execFileSync } from 'node:child_process';
import { join } from 'node:path';
import { checkBoundaries } from './crate-boundaries.mjs';

const manifest = join(import.meta.dirname, '..', '..', '..', 'Cargo.toml');
const metadata = JSON.parse(
  execFileSync(
    'cargo',
    ['metadata', '--no-deps', '--format-version', '1', '--manifest-path', manifest],
    {
      encoding: 'utf8',
    },
  ),
);
const violations = checkBoundaries(metadata);

if (violations.length > 0) {
  for (const violation of violations) {
    console.error(`crate boundary: ${violation}`);
  }
  process.exitCode = 1;
} else {
  console.log(`Crate boundaries hold for ${String(metadata.packages.length)} crates.`);
}
