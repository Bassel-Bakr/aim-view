/**
 * Writes a release's version into every manifest that carries one: the Cargo packages (the core
 * and the workspace members), the desktop app's tauri.conf.json (its installer's version) and
 * ui/package.json. The release workflow (.github/workflows/release.yml) runs it before building;
 * the change is not committed (the release's tag is the record), and cargo brings Cargo.lock
 * along as it builds.
 * Usage: bun scripts/stamp-version.ts <version>
 */
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

/** The repo's root folder. */
const root = join(import.meta.dir, '..');
/** The version to write, X.Y.Z, from the command line. */
const version = process.argv[2] ?? '';
if (!/^\d+\.\d+\.\d+$/.test(version)) {
  console.error(`stamp-version: give a version as X.Y.Z, not "${version}"`);
  process.exit(1);
}

/** A manifest and the pattern of its own version line (the first match is the package's). */
const MANIFESTS: [string, RegExp][] = [
  ...['Cargo.toml', 'browser-service/Cargo.toml', 'desktop/Cargo.toml', 'service/Cargo.toml', 'server/Cargo.toml'].map(
    (file): [string, RegExp] => [file, /^version = "[^"]*"/m],
  ),
  ['desktop/tauri.conf.json', /"version": "[^"]*"/],
  ['ui/package.json', /"version": "[^"]*"/],
];

for (const [file, pattern] of MANIFESTS) {
  const path = join(root, file);
  const text = readFileSync(path, 'utf8');
  if (!pattern.test(text)) {
    console.error(`stamp-version: no version in ${file}`);
    process.exit(1);
  }
  const stamped = text.replace(pattern, (line) => line.replace(/"[^"]*"$/, `"${version}"`));
  writeFileSync(path, stamped);
  console.log(`${file}: ${version}`);
}
