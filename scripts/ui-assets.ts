// Builds the review core for the browser (WebAssembly) and copies what the UI ships beside it into ui/generated/
// (not in git): the core's module (core/aimview.wasm) and the detector models the browser runs (models/, the _u8in
// exports). angular.json serves ui/generated/ as it is; Angular takes no files from outside ui/.
import { $ } from 'bun';
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';

const root = join(import.meta.dir, '..');
const out = join(root, 'ui', 'generated');
rmSync(out, { recursive: true, force: true });

// --release: the shipped build (whole-program optimization, slow to build); else the quick one for development
const profile = process.argv.includes('--release') ? 'release' : 'wasm-dev';
await $`cargo build --profile ${profile} --target wasm32-unknown-unknown`.cwd(root).quiet();
mkdirSync(join(out, 'core'), { recursive: true });
copyFileSync(join(root, `target/wasm32-unknown-unknown/${profile}/aimview.wasm`), join(out, 'core/aimview.wasm'));

// the models the model panel offers (python/model/models.json), each as its _u8in export
const exports = join(root, 'python/model/exports');
const listed = Object.keys(JSON.parse(readFileSync(join(root, 'python/model/models.json'), 'utf8')).models);
mkdirSync(join(out, 'models'), { recursive: true });
const models = listed.map((n) => `detector_${n}_u8in.onnx`).filter((f) => existsSync(join(exports, f)));
for (const f of models) copyFileSync(join(exports, f), join(out, 'models', f));
console.log(`ui/generated: core/aimview.wasm and ${models.length} models`);
