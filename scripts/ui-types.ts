// Makes the TypeScript types of the JSON the UI reads from the Rust structs that write it (ts-rs: the `ts` feature of
// the core and the service), into ui/src/app/generated/ (in git, so the UI builds without Rust). ui/src/app/api.ts
// re-exports them under the UI's names. Run it after changing a Rust struct the UI reads.
//
// Each type with #[ts(export)] has a test that writes its file (only those tests run). ts-rs names a file for its type
// (AmmoRules.ts); the files are then named as Angular's style guide names files (ammo-rules.ts), their imports of each
// other too, and ESLint and Prettier make them follow the UI's rules.
import { $ } from 'bun';
import { readdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const root = join(import.meta.dir, '..');
const out = join(root, 'ui', 'src', 'app', 'generated');

// the folder holds only what the export writes, so a type that is gone from Rust is gone here too
rmSync(out, { recursive: true, force: true });
// i64 is a number in the JSON (counts and frames), not ts-rs's default bigint
const exported = await $`cargo test --profile quick --lib -p aimview -p aimview-service --features aimview-service/ts export_bindings_`
  .cwd(root)
  .env({ ...process.env, TS_RS_EXPORT_DIR: out, TS_RS_LARGE_INT: 'number' })
  .quiet()
  .nothrow();
if (exported.exitCode !== 0) {
  console.error(exported.stdout.toString(), exported.stderr.toString());
  process.exit(exported.exitCode);
}

/** A type's file name: AmmoRules, ammo-rules. */
const kebab = (type: string) => type.replace(/([a-z0-9])([A-Z])/g, '$1-$2').toLowerCase();

for (const file of readdirSync(out)) {
  const text = readFileSync(join(out, file), 'utf8').replace(
    /from "\.\/(\w+)"/g,
    (_, type: string) => `from "./${kebab(type)}"`,
  );
  writeFileSync(join(out, file), text);
  renameSync(join(out, file), join(out, `${kebab(file.replace(/\.ts$/, ''))}.ts`));
}
// the UI's lint rules that ts-rs's output breaks have fixes (an interface for an object type, T[] for Array<T>, Record
// for an index signature); ESLint applies them, and fails on any it cannot fix
await $`bun run eslint --fix src/app/generated`.cwd(join(root, 'ui'));
await $`bun run prettier --write --log-level warn src/app/generated`.cwd(join(root, 'ui'));
