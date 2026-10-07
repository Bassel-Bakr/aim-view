/**
 * Builds the review core for the browser (WebAssembly) and copies what the UI ships beside it into
 * ui/generated/ (not in git): the core's module (core/aimview.wasm), the review service's
 * (service/aimview_service.wasm), the detector models the browser runs (models/, the _u8in
 * exports, each with its settings file detector_<name>.json: python/model/MODEL_FILE.md, and
 * models.json) and the user's area finder data (data/), which browser mode starts from.
 * angular.json serves core/ to every mode and the rest to browser mode only; Angular takes no files
 * from outside ui/. The desktop installer bundles models/ (desktop/tauri.conf.json).
 *
 * Only what the modes asked for is made (--modes, all three by default): the core for every mode, the
 * service and the area data for browser mode, the models for browser mode and the desktop installer.
 * Only what changed is redone: one cargo run builds the core and the service together (their links
 * overlap), cargo rebuilds only after a Rust change, wasm-opt runs only when the service's module or
 * its options changed (its stamp in cargo's target folder), and a file is written only when its
 * bytes differ. Each step's time goes in the costs log (scripts/costs.ts, COSTS.md). The models and
 * the data come from the settings' folders (aimview.json: models, data).
 *
 * The area finder data names the user's recordings, so a build meant for others leaves it out: it
 * is copied unless --no-data, but with --release (the `build*` scripts) only with --data.
 * Usage: bun scripts/ui-assets.ts [--release] [--data | --no-data] [--modes browser,server,desktop]
 * (`bun run assets`)
 */
import { $ } from 'bun';
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { step } from './costs';
import { ROOT, folder } from './local-config';

/** Where the assets go: ui/generated/. */
const out = join(ROOT, 'ui', 'generated');
/** The command line's flags. */
const args = process.argv.slice(2);
/** --release: the shipped build (whole-program optimization, slow to build); else the quick one. */
const release = args.includes('--release');
/** Where --modes is among the flags; -1 when it is not given. */
const modesAt = args.indexOf('--modes');
/** The modes the assets are for (--modes a,b), all three by default. */
const modes = modesAt >= 0 ? args[modesAt + 1].split(',') : ['browser', 'server', 'desktop'];
/** Browser mode runs the service, the detector and the area finder in the page. */
const forBrowser = modes.includes('browser');
/** The desktop installer bundles the models (desktop/tauri.conf.json). */
const withModels = forBrowser || modes.includes('desktop');
/** The cargo profile the WebAssembly builds use. */
const profile = release ? 'release' : 'wasm-dev';
/** Where cargo puts the WebAssembly modules of that profile. */
const built = join(ROOT, 'target', 'wasm32-unknown-unknown', profile);
/** wasm-opt's optimization level: -O2 for --release; the quick build's -O1 gives the same size in half the time. */
const level = release ? '-O2' : '-O1';
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

/** Writes `bytes` to `path` (its folder made) unless the file already holds them; says whether it wrote. */
function writeIfChanged(path: string, bytes: Uint8Array): boolean {
  if (existsSync(path) && Buffer.from(readFileSync(path)).equals(Buffer.from(bytes))) return false;
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, bytes);
  return true;
}

/**
 * Copies the named files from `from` into `to`, and removes the files in `to` that are neither among them nor in
 * `keep` (files written there from elsewhere).
 */
function mirror(from: string, to: string, names: string[], keep: string[] = []): void {
  mkdirSync(to, { recursive: true });
  for (const name of names) writeIfChanged(join(to, name), readFileSync(join(from, name)));
  for (const name of readdirSync(to)) {
    if (!names.includes(name) && !keep.includes(name)) rmSync(join(to, name), { force: true });
  }
}

/**
 * The core as WebAssembly, for every mode (the Crops page; in browser mode the review too), and for
 * browser mode the review service (browser-service/), in one cargo run: the two modules' links run at
 * once (36 s and 66 s apart with the release profile, 2026-10-07), and the core comes out the same.
 */
async function cargo(): Promise<void> {
  const packages = forBrowser ? ['-p', 'aimview', '-p', 'aimview-browser-service'] : ['-p', 'aimview'];
  const what = forBrowser ? 'core and service' : 'core';
  await step(`assets: ${what} wasm (cargo)`, async () => {
    await $`cargo build --profile ${profile} --target wasm32-unknown-unknown ${packages}`.cwd(ROOT).quiet();
  }, { profile });
  writeIfChanged(join(out, 'core', 'aimview.wasm'), readFileSync(join(built, 'aimview.wasm')));
}

/**
 * The review service for browser mode's worker, through Binaryen's Asyncify: its one asynchronous
 * import (host.host_fs, the page's file system) suspends the module while the page answers.
 * wasm-opt keeps the features rustc's output uses (SIMD and the rest of its target_features
 * section). It runs only when its input or its options changed since it last ran.
 */
async function service(): Promise<void> {
  const input = join(built, 'aimview_browser_service.wasm');
  const output = join(out, 'service', 'aimview_service.wasm');
  const options = ['--asyncify', '--pass-arg=asyncify-imports@host.host_fs', level, ...features];
  const stamp = `${Bun.hash(readFileSync(input))} ${options.join(' ')}`;
  const stampFile = `${input}.opt-stamp`;
  if (existsSync(output) && existsSync(stampFile) && readFileSync(stampFile, 'utf8') === stamp) {
    console.log(`  assets: wasm-opt ${level}: its input is unchanged, kept`);
    return;
  }
  mkdirSync(dirname(output), { recursive: true });
  await step('assets: wasm-opt (Asyncify)', async () => {
    const wasmOpt = join(ROOT, 'node_modules', '.bin', 'wasm-opt');
    await $`${wasmOpt} ${input} ${options} -o ${output}`.cwd(ROOT).quiet();
  }, { level });
  writeFileSync(stampFile, stamp);
}

/**
 * The models the model panel offers (models.json beside the models folder), each as its _u8in
 * export and its settings file (a model without one takes today's values), and models.json itself,
 * so the desktop app (which bundles this folder) reads the same models and default.
 */
function models(): void {
  const exports = folder('models');
  const listPath = join(exports, '..', 'models.json');
  const listed = Object.keys(JSON.parse(readFileSync(listPath, 'utf8')).models);
  const exported = listed.filter((name) => existsSync(join(exports, `detector_${name}_u8in.onnx`)));
  const settled = exported.filter((name) => existsSync(join(exports, `detector_${name}.json`)));
  const files = [
    ...exported.map((name) => `detector_${name}_u8in.onnx`),
    ...settled.map((name) => `detector_${name}.json`),
  ];
  mirror(exports, join(out, 'models'), files, ['models.json']);
  writeIfChanged(join(out, 'models', 'models.json'), readFileSync(listPath));
  const unexported = listed.filter((name) => !exported.includes(name));
  const unsettled = exported.filter((name) => !settled.includes(name));
  console.log(`ui/generated: ${exported.length} models`);
  if (unexported.length) console.log(`  listed with no _u8in export in ${exports}: ${unexported.join(', ')}`);
  if (unsettled.length) console.log(`  with no settings file (detector_<name>.json): ${unsettled.join(', ')}`);
}

/**
 * The area finder's training data, from the data folder in Python's layout (its vod_app/, as the
 * review server keeps it there), or none: a build for others leaves it out.
 */
function data(): void {
  const from = join(folder('data'), 'vod_app');
  const files = ['area_examples.jsonl', 'area_kinds.json'].filter((name) => existsSync(join(from, name)));
  if (!args.includes('--data') && (release || args.includes('--no-data'))) {
    rmSync(join(out, 'data'), { recursive: true, force: true });
    console.log('  no area finder data (data/)');
    return;
  }
  mirror(from, join(out, 'data'), files);
  console.log(`  area finder data (data/): ${files.join(', ') || `none in ${from}`}`);
}

await step('assets', async () => {
  // what a mode does not need is left as it is: angular.json gives each mode only its own
  await cargo();
  if (forBrowser) await service();
  if (withModels) models();
  if (forBrowser) data();
}, { profile, modes: modes.join(',') });
