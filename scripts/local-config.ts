/**
 * Aim View's settings for this checkout and this computer, as src/local_config.rs and python/local_config.py read
 * them: aimview.defaults.json (in git) under aimview.json (out of git, optional), both at the repo's root, each
 * top-level key of the second replacing the first's. Relative paths start at the repo's root.
 * In: the two files. Out: the folders the bun scripts use (the data and models folders).
 */
import { existsSync, readFileSync } from 'node:fs';
import { isAbsolute, join } from 'node:path';

/** The repo's root, which holds both files. */
export const ROOT = join(import.meta.dir, '..');

/** The settings as JSON: folder keys to paths ("/" between names), or null where a setting is not given. */
type Settings = Record<string, unknown>;

/** The defaults with this computer's settings over them. */
function settings(): Settings {
  const read = (name: string): Settings => JSON.parse(readFileSync(join(ROOT, name), 'utf8'));
  const local = join(ROOT, 'aimview.json');
  return { ...read('aimview.defaults.json'), ...(existsSync(local) ? read('aimview.json') : {}) };
}

/** A folder the settings name (data, models, ui, ffmpeg, vods), as an absolute path; it fails when it is not set. */
export function folder(key: string): string {
  const path = settings()[key];
  if (typeof path !== 'string') {
    throw new Error(`no ${key} folder: name it in aimview.json at the repo's root (see aimview.defaults.json)`);
  }
  return isAbsolute(path) ? path : join(ROOT, path);
}
