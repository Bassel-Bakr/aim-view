/**
 * Keeps Cargo's build folder (target/) under a size. Cargo never deletes an old build output, so each change of a
 * dependency, a feature or a flag leaves another copy behind (58 GB by 2026-10-11). This runs cargo sweep, which deletes
 * the least recently used outputs until the folder is under the settings' build_cache.max_gb, at most once every
 * build_cache.every_hours; a stamp file in the folder holds the last sweep's time. In: the settings and the build
 * folder. Out: the folder, trimmed, and its stamp. The post-commit hook runs it (lefthook.yml); `bun run sweep` runs it
 * now, whatever the stamp says.
 * Usage: bun scripts/sweep.ts [--now]
 */
import { existsSync, statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { ROOT, setting } from './local-config';

/** Cargo's build folder: CARGO_TARGET_DIR, or Cargo's own default under the repo's root. */
const TARGET = process.env.CARGO_TARGET_DIR ?? join(ROOT, 'target');
/** The file whose change time is the last sweep's. */
const STAMP = join(TARGET, '.last-sweep');
/** Milliseconds in an hour. */
const MS_PER_HOUR = 3_600_000;

/** The build folder's limits (aimview.defaults.json's build_cache). */
interface BuildCache {
  /** The size the sweep trims the folder to, in gigabytes. */
  max_gb: number;
  /** The least time between two sweeps, in hours. */
  every_hours: number;
}

/** Whether the last sweep is at least `everyHours` hours old, or there was none. */
function sweepDue(everyHours: number): boolean {
  return !existsSync(STAMP) || Date.now() - statSync(STAMP).mtimeMs >= everyHours * MS_PER_HOUR;
}

/** Whether cargo-sweep is installed. */
function sweepInstalled(): boolean {
  return Bun.spawnSync(['cargo', 'sweep', '--version'], { stdout: 'ignore', stderr: 'ignore' }).exitCode === 0;
}

if (import.meta.main) {
  const limits = setting('build_cache') as BuildCache;
  const now = process.argv.includes('--now');
  if (!existsSync(TARGET) || (!now && !sweepDue(limits.every_hours))) process.exit(0);
  if (!sweepInstalled()) {
    console.error('sweep: cargo-sweep is not installed (cargo install cargo-sweep), so target/ was not trimmed');
    process.exit(0);
  }
  const sweep = Bun.spawnSync(['cargo', 'sweep', '--maxsize', `${limits.max_gb}GB`], {
    cwd: ROOT,
    stdout: 'inherit',
    stderr: 'inherit',
  });
  if (sweep.exitCode === 0) writeFileSync(STAMP, `${new Date().toISOString()}\n`);
  process.exit(sweep.exitCode);
}
