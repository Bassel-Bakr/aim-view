/**
 * What the project's commands cost, kept so no one runs a command again only to learn how long it takes (docs/COSTS.md).
 * The build scripts time their own steps with `step`; `bun scripts/costs.ts run <name> [--config a=b,c=d] --
 * <command ...>` times any other command. Each timing is one JSON line in costs.jsonl in the data folder
 * (aimview.json's data), with the configuration it ran in (a cargo profile, the modes built, a model, a device, the
 * frames at once...): the same step costs differently in each. Every timing rewrites docs/COSTS.md's measured table from
 * that log: each step and configuration's latest time, its median over its last runs, and how many runs there were.
 * `bun run costs` rewrites it by hand.
 * In: the steps and commands timed, and the log. Out: the log, and docs/COSTS.md's measured table.
 */
import { $ } from 'bun';
import { appendFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { ROOT, folder } from './local-config';

/** The log of every timing, one JSON line each. */
const LOG = join(folder('data'), 'costs.jsonl');
/** The notes the measured table is written into, between its two markers. */
const COSTS = join(ROOT, 'docs/COSTS.md');
/** The marker before the measured table in docs/COSTS.md. */
const START = '<!-- costs:start -->';
/** The marker after it. */
const END = '<!-- costs:end -->';
/** How many of a step's latest runs its median is taken over. */
const RECENT_RUNS = 5;
/** Milliseconds in a second. */
const MS_PER_S = 1000;

/** What a step ran with, which changes its cost: setting name to value (`profile: release`, `model: full_v3`). */
export type Configuration = Record<string, string>;

/** One timing: the step, its configuration, its seconds, whether it worked, and the commit it ran at. */
interface Timing {
  /** The step's name, as docs/COSTS.md lists it. */
  step: string;
  /** What it ran with; empty when nothing about it changes its cost. */
  config: Configuration;
  /** How long it took, in seconds (a tenth). */
  seconds: number;
  /** Whether it worked. */
  ok: boolean;
  /** The commit checked out when it ran. */
  commit: string;
  /** Whether the checkout had uncommitted changes then. */
  changed: boolean;
  /** When it ended (ISO 8601). */
  at: string;
}

/** The commit checked out (its short hash), and whether the checkout has uncommitted changes. */
type Checkout = [commit: string, changed: boolean];

/** The commit checked out, and whether the checkout has uncommitted changes. */
async function checkout(): Promise<Checkout> {
  const head = await $`git rev-parse --short HEAD`.cwd(ROOT).quiet().nothrow();
  const status = await $`git status --porcelain --untracked-files=no`.cwd(ROOT).quiet().nothrow();
  return [head.stdout.toString().trim(), status.stdout.toString().trim() !== ''];
}

/** A configuration as the table shows it ("profile release, modes browser"); a dash for none. */
function configText(config: Configuration): string {
  const settings = Object.entries(config).map(([name, value]) => `${name} ${value}`);
  return settings.length ? settings.join(', ') : '-';
}

/** Adds a timing to the log, says it on the console, and brings docs/COSTS.md's table up to date. */
async function record(step: string, config: Configuration, seconds: number, ok: boolean): Promise<void> {
  const [commit, changed] = await checkout();
  const at = new Date().toISOString();
  const timing: Timing = { step, config, seconds: Math.round(seconds * 10) / 10, ok, commit, changed, at };
  mkdirSync(dirname(LOG), { recursive: true });
  appendFileSync(LOG, JSON.stringify(timing) + '\n');
  console.log(`  ${step} (${configText(config)}): ${timing.seconds} s${ok ? '' : ' (failed)'}`);
  // the notes are kept current by every timing, not only when someone remembers to update them
  writeTable();
}

/** Runs one step of a script in a configuration, timed into the log (whether it works or fails). */
export async function step<T>(name: string, run: () => Promise<T>, config: Configuration = {}): Promise<T> {
  const started = performance.now();
  let ok = false;
  try {
    const out = await run();
    ok = true;
    return out;
  } finally {
    await record(name, config, (performance.now() - started) / MS_PER_S, ok);
  }
}

/** The median of some seconds. */
function median(values: number[]): number {
  const sorted = [...values].sort((a, b) => a - b);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
}

/** A timing's row: its step and its configuration. */
function rowKey(timing: Timing): string {
  return `${timing.step} | ${configText(timing.config ?? {})}`;
}

/** The measured table: one row per step and configuration, from the log's runs that worked. */
function table(): string {
  const lines = existsSync(LOG) ? readFileSync(LOG, 'utf8').split('\n').filter(Boolean) : [];
  const runs = lines.map((line) => JSON.parse(line) as Timing).filter((timing) => timing.ok);
  const keys = [...new Set(runs.map(rowKey))].sort();
  const rows = keys.map((key) => {
    const own = runs.filter((timing) => rowKey(timing) === key);
    const last = own[own.length - 1];
    const recent = median(own.slice(-RECENT_RUNS).map((timing) => timing.seconds));
    const at = `${last.at.slice(0, 10)}, ${last.commit}${last.changed ? '+' : ''}`;
    return `| ${key} | ${last.seconds} s | ${recent.toFixed(1)} s | ${own.length} | ${at} |`;
  });
  const head =
    `| Step | Configuration | Latest | Median of the last ${RECENT_RUNS} | Runs | ` +
    'Latest run (date, commit; + uncommitted changes) |';
  return [head, '| --- | --- | --- | --- | --- | --- |', ...rows].join('\n');
}

/** Rewrites the measured table in docs/COSTS.md from the log. */
function writeTable(): void {
  const text = readFileSync(COSTS, 'utf8');
  const [before, rest] = text.split(START);
  const after = rest?.split(END)[1];
  if (after === undefined) throw new Error(`docs/COSTS.md has no ${START} ... ${END} markers`);
  writeFileSync(COSTS, `${before}${START}\n${table()}\n${END}${after}`);
}

/** Times a command (its output shown as it runs) into the log in a configuration, and exits with its code. */
async function runTimed(name: string, config: Configuration, command: string[]): Promise<void> {
  let code = 1;
  const run = async (): Promise<void> => {
    code = await Bun.spawn(command, { cwd: process.cwd(), stdio: ['inherit', 'inherit', 'inherit'] }).exited;
    if (code !== 0) throw new Error(`${command.join(' ')} exited with ${code}`);
  };
  await step(name, run, config).catch((error: Error) => console.error(error.message));
  process.exit(code);
}

/** The configuration given as `--config name=value,name=value` among `args`; none when it is not given. */
function configArg(args: string[]): Configuration {
  const at = args.indexOf('--config');
  if (at < 0) return {};
  return Object.fromEntries(args[at + 1].split(',').map((pair) => pair.split('=', 2)));
}

if (import.meta.main) {
  const args = process.argv.slice(2);
  if (args[0] === 'run') {
    const split = args.indexOf('--');
    if (split < 2 || split === args.length - 1) {
      throw new Error('usage: bun scripts/costs.ts run <name> [--config a=b,c=d] -- <command ...>');
    }
    const head = args.slice(1, split);
    const at = head.indexOf('--config');
    await runTimed((at < 0 ? head : head.slice(0, at)).join(' '), configArg(head), args.slice(split + 1));
  }
  writeTable();
}
