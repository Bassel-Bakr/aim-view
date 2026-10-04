// Renames fields in TypeScript where the compiler reports them missing, after the type's fields were renamed (in Rust,
// then `bun run types`, or by hand):
//
//   bun scripts/rename-ts-fields.ts [--project ui/tsconfig.spec.json] [--compiler ngc|tsc] Type.old=new ...
//
// Type is the type the compiler names, or one that extends it (Measure, TimedFlick), and `*` matches any type. Each
// rename is made at the exact place the compiler reports: a property read (TS2339, TS2551) or an object literal's key
// (TS2353). It runs the compiler again until nothing more changes, then lists the errors left for a hand fix (a
// literal cast with `as`, a shorthand key). Angular's ngc, the default, checks the templates too; tsc does not.
import { dirname, join, resolve } from 'node:path';

interface Rename {
  type: string;
  from: string;
  to: string;
}

interface Missing {
  file: string;
  line: number;
  column: number;
  property: string;
  type: string;
}

/** An error's place, as tsc writes it (`file(1,2): error`) or as ngc does (`file:1:2 - error`), and its message. */
const ERROR = /^(.+?)(?:\((\d+),(\d+)\):|:(\d+):(\d+) -) error (TS\d+): (.*)$/;
const MISSING_CODES = ['TS2339', 'TS2551', 'TS2353'];
const MISSING =
  /^(?:Property|Object literal may only specify known properties, and) '(\w+)' does not exist (?:on|in) type '([^']+)'/;

function parseRename(text: string): Rename {
  const match = /^([\w*]+)\.(\w+)=(\w+)$/.exec(text);
  if (!match) throw new Error(`not Type.old=new: ${text}`);
  return { type: match[1], from: match[2], to: match[3] };
}

function compile(project: string, compiler: string): string[] {
  const command = [process.execPath, 'x', compiler, '-p', project, '--noEmit'];
  const run = Bun.spawnSync(command, { cwd: dirname(project) });
  const text = run.stdout.toString() + run.stderr.toString();
  return text
    .replace(/\x1b\[[0-9;]*m/g, '')
    .split(/\r?\n/)
    .filter((line) => ERROR.test(line));
}

function parseMissing(root: string, error: string): Missing | undefined {
  const place = ERROR.exec(error);
  if (!place || !MISSING_CODES.includes(place[6])) return undefined;
  const found = MISSING.exec(place[7]);
  if (!found) return undefined;
  const [line, column] = place[2] ? [place[2], place[3]] : [place[4], place[5]];
  return { file: join(root, place[1]), line: +line, column: +column, property: found[1], type: found[2] };
}

function renameFor(missing: Missing, renames: Rename[]): Rename | undefined {
  const named = (type: string) => type === '*' || new RegExp(`\\b${type}\\b`).test(missing.type);
  return renames.find((rename) => rename.from === missing.property && named(rename.type));
}

/** Makes every rename the errors ask for, last place first in each line; the number made. */
async function renameAll(root: string, errors: string[], renames: Rename[]): Promise<number> {
  const edits = new Map<string, Missing[]>();
  for (const error of errors) {
    const missing = parseMissing(root, error);
    if (missing && renameFor(missing, renames)) edits.set(missing.file, [...(edits.get(missing.file) ?? []), missing]);
  }
  let made = 0;
  for (const [file, places] of edits) {
    const lines = (await Bun.file(file).text()).split('\n');
    places.sort((a, b) => a.line - b.line || b.column - a.column);
    for (const place of places) {
      const text = lines[place.line - 1];
      const at = place.column - 1;
      if (text.slice(at, at + place.property.length) !== place.property) continue;
      const to = renameFor(place, renames)!.to;
      lines[place.line - 1] = text.slice(0, at) + to + text.slice(at + place.property.length);
      made++;
    }
    await Bun.write(file, lines.join('\n'));
    console.log(`${places.length} in ${file}`);
  }
  return made;
}

async function main(): Promise<number> {
  const args = process.argv.slice(2);
  const at = args.indexOf('--project');
  const project = resolve(at >= 0 ? args.splice(at, 2)[1] : 'ui/tsconfig.spec.json');
  const by = args.indexOf('--compiler');
  const compiler = by >= 0 ? args.splice(by, 2)[1] : 'ngc';
  const renames = args.map(parseRename);
  if (renames.length === 0) {
    console.error('usage: bun scripts/rename-ts-fields.ts [--project <tsconfig>] [--compiler ngc|tsc] Type.old=new ...');
    return 2;
  }
  let errors = compile(project, compiler);
  while ((await renameAll(dirname(project), errors, renames)) > 0) errors = compile(project, compiler);
  for (const error of errors) console.log(`left: ${error}`);
  console.log(errors.length ? `${errors.length} errors left` : 'no errors left');
  return errors.length ? 1 : 0;
}

process.exit(await main());
