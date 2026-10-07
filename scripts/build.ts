/**
 * Builds the UI for one mode or several (`bun run build`, `bun run build:<mode>`): the shared assets once
 * (ui-assets.ts --release: the core and the browser's service as WebAssembly, the models), then each mode's Angular
 * build (angular.json: production plus the mode's configuration), all at once when there are several, each in its
 * own process. Each step's time goes in the costs log (scripts/costs.ts, COSTS.md).
 * In: the modes asked for. Out: ui/generated/ and ui/dist/<mode>/.
 * Usage: bun scripts/build.ts [browser] [server] [desktop] [--one-at-a-time] [--data]   (no mode: all three)
 */
import { join } from 'node:path';
import { step } from './costs';
import { ROOT } from './local-config';

/** The modes a build can be for (angular.json's configurations). */
const MODES = ['browser', 'server', 'desktop'];
/** The command line's flags and modes. */
const args = process.argv.slice(2);
/** The modes named on the command line. */
const asked = args.filter((arg) => MODES.includes(arg));
/** The modes to build: those named, or all of them when none is. */
const modes = asked.length ? asked : MODES;
/** Whether the Angular builds run one after another instead of all at once. */
const oneAtATime = args.includes('--one-at-a-time');

/** Runs a command; its output is shown when it ends, so builds running at once do not mix their lines. */
async function run(command: string[], cwd: string): Promise<void> {
  const child = Bun.spawn(command, { cwd, stdout: 'pipe', stderr: 'pipe' });
  const [out, err, code] = await Promise.all([
    new Response(child.stdout).text(),
    new Response(child.stderr).text(),
    child.exited,
  ]);
  process.stdout.write(out);
  process.stderr.write(err);
  if (code !== 0) throw new Error(`${command.join(' ')} exited with ${code}`);
}

/** One mode's Angular build, timed. */
function angular(mode: string): Promise<void> {
  const command = ['bun', 'run', 'build', '--configuration', `production,${mode}`];
  return step('build: Angular (production)', () => run(command, join(ROOT, 'ui')), { mode });
}

/** The assets the modes need (ui-assets.ts, release), timed there step by step. */
async function assets(): Promise<void> {
  const data = args.includes('--data') ? ['--data'] : [];
  const command = ['bun', 'scripts/ui-assets.ts', '--release', '--modes', modes.join(','), ...data];
  const child = Bun.spawn(command, { cwd: ROOT, stdio: ['inherit', 'inherit', 'inherit'] });
  if ((await child.exited) !== 0) throw new Error('the assets failed');
}

/** Whether the Angular builds run at once. */
const together = modes.length > 1 && !oneAtATime;
await step('build', async () => {
  await assets();
  if (together) {
    await Promise.all(modes.map(angular));
  } else {
    for (const mode of modes) await angular(mode);
  }
}, { modes: modes.join(','), ...(modes.length > 1 ? { angular: together ? 'at once' : 'one at a time' } : {}) });
