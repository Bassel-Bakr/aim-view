// Whether two JSON files, or every JSON file in two folders, hold the same values once keys in the first are renamed:
// the check that a change to a format moved only names (a new baseline after a rename, BENCH.md).
//
//   bun scripts/same-json.ts <old file or folder> <new file or folder> [path=name ...]
//
// A path leads to a key, with [] for every element of an array: flicks[].n=kill_number, or [].dir=direction_deg in a
// list. Numbers must be equal exactly, and both sides must have the same keys. It prints every difference and exits
// with 1 when there is one.
import { readdirSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';

type Json = null | boolean | number | string | Json[] | JsonObject;

interface JsonObject {
  [key: string]: Json;
}

interface Rename {
  steps: string[];
  name: string;
}

function parseRename(text: string): Rename {
  const [path, name] = text.split('=');
  if (!path || !name) throw new Error(`not path=name: ${text}`);
  return { steps: path.split('.'), name };
}

function isObject(value: Json): value is JsonObject {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

/** Renames the key `steps` leads to, in place; a path that leads nowhere changes nothing. */
function renameKey(value: Json, steps: string[], name: string): void {
  const [step, ...rest] = steps;
  if (rest.length === 0) {
    if (isObject(value) && step in value) {
      value[name] = value[step];
      delete value[step];
    }
    return;
  }
  const every = step.endsWith('[]');
  const key = every ? step.slice(0, -2) : step;
  const child = key === '' ? value : isObject(value) ? value[key] : undefined;
  if (child === undefined) return;
  if (every) {
    if (Array.isArray(child)) for (const element of child) renameKey(element, rest, name);
  } else {
    renameKey(child, rest, name);
  }
}

/** Every place where `got` differs from `want`, as JSON paths. */
function differences(path: string, got: Json, want: Json): string[] {
  if (Array.isArray(got) && Array.isArray(want)) {
    if (got.length !== want.length) return [`${path}: ${got.length} elements against ${want.length}`];
    return got.flatMap((element, i) => differences(`${path}[${i}]`, element, want[i]));
  }
  if (isObject(got) && isObject(want)) {
    const keys = new Set([...Object.keys(got), ...Object.keys(want)]);
    return [...keys].flatMap((key) => {
      if (!(key in got)) return [`${path}.${key}: only in the new one`];
      if (!(key in want)) return [`${path}.${key}: only in the old one`];
      return differences(`${path}.${key}`, got[key], want[key]);
    });
  }
  return got === want ? [] : [`${path}: ${JSON.stringify(got)} against ${JSON.stringify(want)}`];
}

/** The JSON files under a folder, as paths relative to it; a file gives itself. */
function jsonFiles(root: string): string[] {
  if (!statSync(root).isDirectory()) return [''];
  return readdirSync(root, { recursive: true, encoding: 'utf8' })
    .filter((path) => path.endsWith('.json'))
    .sort();
}

async function main(): Promise<number> {
  const [oldRoot, newRoot, ...renameTexts] = process.argv.slice(2);
  if (!oldRoot || !newRoot) {
    console.error('usage: bun scripts/same-json.ts <old file or folder> <new file or folder> [path=name ...]');
    return 2;
  }
  const renames = renameTexts.map(parseRename);
  const oldFiles = jsonFiles(oldRoot);
  const newFiles = new Set(jsonFiles(newRoot));
  let wrong = 0;
  for (const file of oldFiles) {
    const label = file || relative('.', oldRoot);
    if (!newFiles.has(file)) {
      console.log(`${label}: missing in the new one`);
      wrong++;
      continue;
    }
    newFiles.delete(file);
    const old: Json = await Bun.file(join(oldRoot, file)).json();
    for (const { steps, name } of renames) renameKey(old, steps, name);
    const found = differences('', await Bun.file(join(newRoot, file)).json(), old);
    for (const line of found.slice(0, 20)) console.log(`${label}${line}`);
    if (found.length > 20) console.log(`${label}: ${found.length - 20} more`);
    wrong += found.length;
  }
  for (const file of newFiles) {
    console.log(`${file}: only in the new one`);
    wrong++;
  }
  console.log(wrong ? `${wrong} differences` : `${oldFiles.length} files hold the same values`);
  return wrong ? 1 : 0;
}

process.exit(await main());
