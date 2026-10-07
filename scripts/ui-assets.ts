/**
 * Builds the review core for the browser (WebAssembly) and copies what the UI ships beside it into
 * ui/generated/ (not in git): the core's module (core/aimview.wasm), the review service's
 * (service/aimview_service.wasm), the detector models the browser runs (models/, the _u8in
 * exports, each with its settings file detector_<name>.json: python/model/MODEL_FILE.md, and
 * models.json) and the user's area finder data (data/), which browser mode starts from.
 * angular.json serves ui/generated/ as it is; Angular takes no files from outside ui/.
 *
 * The area finder data names the user's recordings, so a build meant for others leaves it out: it
 * is copied unless --no-data, but with --release (the `build:*` scripts) only with --data.
 * Usage: bun scripts/ui-assets.ts [--release] [--data | --no-data] (`bun run assets`)
 */
import { $ } from 'bun';
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';

/** The repo's root folder. */
const root = join(import.meta.dir, '..');
/** Where the assets go: ui/generated/, emptied first. */
const out = join(root, 'ui', 'generated');
/** The command line's flags. */
const args = process.argv.slice(2);
rmSync(out, { recursive: true, force: true });

/** --release: the shipped build (whole-program optimization, slow to build); else the quick one. */
const release = args.includes('--release');
/** The cargo profile the WebAssembly builds use. */
const profile = release ? 'release' : 'wasm-dev';
await $`cargo build --profile ${profile} --target wasm32-unknown-unknown`.cwd(root).quiet();
mkdirSync(join(out, 'core'), { recursive: true });
copyFileSync(join(root, `target/wasm32-unknown-unknown/${profile}/aimview.wasm`), join(out, 'core/aimview.wasm'));

// the review service (browser-service/) for browser mode's worker, through Binaryen's Asyncify: its one asynchronous
// import (host.host_fs, the page's file system) suspends the module while the page answers. wasm-opt keeps the
// features rustc's output uses (SIMD and the rest of its target_features section). -O2 for --release; the quick
// build's -O1 gives the same size in about half the time (37 s, not 67).
await $`cargo build --profile ${profile} --target wasm32-unknown-unknown -p aimview-browser-service`.cwd(root).quiet();
/** wasm-opt's optimization level. */
const level = release ? '-O2' : '-O1';
mkdirSync(join(out, 'service'), { recursive: true });
/** The WebAssembly features rustc's output uses, which wasm-opt must keep. */
const features = [
  '--enable-simd',
  '--enable-bulk-memory',
  '--enable-nontrapping-float-to-int',
  '--enable-sign-ext',
  '--enable-mutable-globals',
  '--enable-multivalue',
  '--enable-reference-types',
];
await $`${join(root, 'node_modules', '.bin', 'wasm-opt')} ${join(root, `target/wasm32-unknown-unknown/${profile}/aimview_browser_service.wasm`)} --asyncify --pass-arg=asyncify-imports@host.host_fs ${level} ${features} -o ${join(out, 'service', 'aimview_service.wasm')}`
  .cwd(root)
  .quiet();

// the models the model panel offers (python/model/models.json), each as its _u8in export and its settings file (a
// model without one takes today's values)
/** Where the models' exports are. */
const exports = join(root, 'python/model/exports');
/** The models models.json lists, by name. */
const listed = Object.keys(JSON.parse(readFileSync(join(root, 'python/model/models.json'), 'utf8')).models);
mkdirSync(join(out, 'models'), { recursive: true });
/** The listed models with a _u8in export. */
const exported = listed.filter((n) => existsSync(join(exports, `detector_${n}_u8in.onnx`)));
/** The exported models with a settings file. */
const settled = exported.filter((n) => existsSync(join(exports, `detector_${n}.json`)));
for (const f of [
  ...exported.map((n) => `detector_${n}_u8in.onnx`),
  ...settled.map((n) => `detector_${n}.json`),
]) {
  copyFileSync(join(exports, f), join(out, 'models', f));
}
// the list itself, so the desktop app (which bundles this folder) reads the same models and default
copyFileSync(join(root, 'python/model/models.json'), join(out, 'models', 'models.json'));
/** The listed models with no export, which the browser cannot offer. */
const unexported = listed.filter((n) => !exported.includes(n));
/** The exported models with no settings file. */
const unsettled = exported.filter((n) => !settled.includes(n));
console.log(`ui/generated: core/aimview.wasm, service/aimview_service.wasm and ${exported.length} models`);
if (unexported.length) console.log(`  listed with no _u8in export in python/model/exports: ${unexported.join(', ')}`);
if (unsettled.length) console.log(`  with no settings file (detector_<name>.json): ${unsettled.join(', ')}`);

// the area finder's training data, from python/server.py's data folder (test_out/vod_app/), as it is there
/** The data folder the area finder's training data is copied from. */
const from = join(root, 'test_out', 'vod_app');
/** The training data files that are there. */
const files = ['area_examples.jsonl', 'area_kinds.json'].filter((f) => existsSync(join(from, f)));
if (!args.includes('--data') && (release || args.includes('--no-data'))) {
  console.log('  no area finder data (data/)');
} else {
  mkdirSync(join(out, 'data'), { recursive: true });
  for (const f of files) copyFileSync(join(from, f), join(out, 'data', f));
  console.log(`  area finder data (data/): ${files.join(', ') || 'none in test_out/vod_app/'}`);
}
