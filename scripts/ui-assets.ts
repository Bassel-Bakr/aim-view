// Builds the review core for the browser (WebAssembly) and copies what the UI ships beside it into ui/generated/
// (not in git): the core's module (core/aimview.wasm), the detector models the browser runs (models/, the _u8in
// exports, each with its settings file detector_<name>.json: python/model/MODEL_FILE.md) and the user's area finder
// data (data/), which browser mode starts from. angular.json serves ui/generated/
// as it is; Angular takes no files from outside ui/.
//
// The area finder data names the user's recordings, so a build meant for others leaves it out: it is copied unless
// --no-data, but with --release (the `build:*` scripts) only with --data.
import { $ } from 'bun';
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';

const root = join(import.meta.dir, '..');
const out = join(root, 'ui', 'generated');
const args = process.argv.slice(2);
rmSync(out, { recursive: true, force: true });

// --release: the shipped build (whole-program optimization, slow to build); else the quick one for development
const release = args.includes('--release');
const profile = release ? 'release' : 'wasm-dev';
await $`cargo build --profile ${profile} --target wasm32-unknown-unknown`.cwd(root).quiet();
mkdirSync(join(out, 'core'), { recursive: true });
copyFileSync(join(root, `target/wasm32-unknown-unknown/${profile}/aimview.wasm`), join(out, 'core/aimview.wasm'));

// the models the model panel offers (python/model/models.json), each as its _u8in export and its settings file (a
// model without one takes today's values)
const exports = join(root, 'python/model/exports');
const listed = Object.keys(JSON.parse(readFileSync(join(root, 'python/model/models.json'), 'utf8')).models);
mkdirSync(join(out, 'models'), { recursive: true });
const exported = listed.filter((n) => existsSync(join(exports, `detector_${n}_u8in.onnx`)));
const settled = exported.filter((n) => existsSync(join(exports, `detector_${n}.json`)));
for (const f of [
  ...exported.map((n) => `detector_${n}_u8in.onnx`),
  ...settled.map((n) => `detector_${n}.json`),
]) {
  copyFileSync(join(exports, f), join(out, 'models', f));
}
// the list itself, so the desktop app (which bundles this folder) reads the same models and default
copyFileSync(join(root, 'python/model/models.json'), join(out, 'models', 'models.json'));
const unexported = listed.filter((n) => !exported.includes(n));
const unsettled = exported.filter((n) => !settled.includes(n));
console.log(`ui/generated: core/aimview.wasm and ${exported.length} models`);
if (unexported.length) console.log(`  listed with no _u8in export in python/model/exports: ${unexported.join(', ')}`);
if (unsettled.length) console.log(`  with no settings file (detector_<name>.json): ${unsettled.join(', ')}`);

// the area finder's training data, from python/server.py's data folder (test_out/vod_app/), as it is there
const from = join(root, 'test_out', 'vod_app');
const files = ['area_examples.jsonl', 'area_kinds.json'].filter((f) => existsSync(join(from, f)));
if (!args.includes('--data') && (release || args.includes('--no-data'))) {
  console.log('  no area finder data (data/)');
} else {
  mkdirSync(join(out, 'data'), { recursive: true });
  for (const f of files) copyFileSync(join(from, f), join(out, 'data', f));
  console.log(`  area finder data (data/): ${files.join(', ') || 'none in test_out/vod_app/'}`);
}
