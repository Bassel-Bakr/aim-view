import { HttpRequest } from '@angular/common/http';
import { ResourceRef } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { ApiRoutes } from '../fake-api';
import { mouseRun } from '../fake-mouse';
import { MouseMeasures } from '../mouse-api';
import { MODE_CASES, ModeCase, setUp } from './contract-case';
import { MouseLogs } from './mouse-logs';
import { RecordingSource } from './recording-source';
import { StatsFiles } from './stats-files';

const NAME = 'Air - 1 - 2026.10.01-16.23.03.mp4';
const STATS_NAME = 'Air - Challenge - 2026.10.01-16.23.03 Stats.csv';
const STATS = 'Kill #,Timestamp\n1,16:22:20.833\n\nKills:,1\nScore:,1\nScenario:,Air\n';
const LOG_NAME = 'mouse_2026-10-01_16-22-00.bin';
const NO_STATS =
  "The recording has no stats file: a log is matched with the run by the stats file's kill times.";

/**
 * A review server that takes the recording added and its stats file, and (the browser mode's review service, as the
 * desktop app's) keeps the mouse logs sent to it and measures the run from the one kept, once the run has its stats
 * file; `covers` says whether the log covers the run (the server mode reads no logs).
 */
function fakeServer(covers = true): ApiRoutes {
  let stats = false;
  let log: string | null = null;
  return {
    '/api/vods': [],
    '/api/upload': (req: HttpRequest<unknown>) => {
      const name = req.params.get('name') ?? '';
      const id = req.params.get('id');
      if (!id) return { id: `uploads/${name}`, saved: name };
      stats = true;
      return { id, saved: name, job: { stage: 'none' }, stats: true };
    },
    '/api/mouse_log': (req: HttpRequest<unknown>) => {
      log = req.params.get('name');
      return { saved: log };
    },
    '/api/mouse': (): MouseMeasures | null => {
      if (log === null) return null;
      if (!stats) return { file: log, run: null, error: NO_STATS };
      if (!covers) return { file: log, run: null, error: 'the log does not cover this run' };
      return { file: log, run: mouseRun(), error: null };
    },
    [`/files/data/mouse/${LOG_NAME}`]: (req: HttpRequest<unknown>) => {
      if (req.method === 'DELETE') log = null;
      return null;
    },
  };
}

/** The resource's value once it has one, while the fake server answers. */
async function settled(
  mode: ModeCase,
  ref: ResourceRef<MouseMeasures | null | undefined>,
  routes: ApiRoutes,
): Promise<MouseMeasures | null> {
  const read = (async () => {
    for (;;) {
      TestBed.tick();
      await new Promise((r) => setTimeout(r));
      if (ref.error()) throw ref.error();
      if (ref.hasValue() && !ref.isLoading()) return ref.value() ?? null;
    }
  })();
  return mode.finish(read, routes);
}

/** A log's header and nothing else: it started at the given time. */
function logStartingAt(ms: number): File {
  const head = new Uint8Array(32);
  new DataView(head.buffer).setBigInt64(24, BigInt(ms) * 1_000_000n, true);
  return new File([head], LOG_NAME);
}

for (const mode of MODE_CASES) {
  describe(`MouseLogs (${mode.name} mode)`, () => {
    async function added(routes: ApiRoutes): Promise<string> {
      const source = TestBed.inject(RecordingSource);
      return (await mode.finish(source.add([new File(['v'], NAME)]), routes)).ids[0];
    }

    it('measures a recording that has no log as none, and logs nothing itself', async () => {
      const logs = setUp(mode, MouseLogs);
      const routes = fakeServer();
      const id = await added(routes);
      const measures = TestBed.runInInjectionContext(() => logs.measures(() => id));
      expect(await settled(mode, measures, routes)).toBeNull();
      expect(logs.logs).toBe(false);
    });

    it(
      mode.name === 'browser'
        ? 'reads a log added for the run, and forgets it'
        : 'turns down a log',
      async () => {
        const logs = setUp(mode, MouseLogs);
        const routes = fakeServer();
        const id = await added(routes);
        const log = logStartingAt(Date.UTC(2026, 9, 1, 13, 22));
        if (!logs.adds) {
          await expect(logs.add(id, log)).rejects.toThrow(/no mouse logs/);
          return;
        }
        await expect(mode.finish(logs.add(id, log), routes)).rejects.toThrow(/no stats file/);
        const stats = TestBed.inject(StatsFiles);
        await mode.finish(stats.pairFile(id, new File([STATS], STATS_NAME)), routes);
        const measured = await mode.finish(logs.add(id, log), routes);
        expect(measured.run?.matched).toBe(2);
        const measures = TestBed.runInInjectionContext(() => logs.measures(() => id));
        expect((await settled(mode, measures, routes))?.file).toBe(LOG_NAME);
        await mode.finish(logs.forget(id), routes);
        measures.reload();
        expect(await settled(mode, measures, routes)).toBeNull();
      },
    );
  });
}

describe('MouseLogs: what the browser turns down', () => {
  it('a log that does not cover the run, and says why', async () => {
    const [browser] = MODE_CASES;
    const logs = setUp(browser, MouseLogs);
    const routes = fakeServer(false);
    const source = TestBed.inject(RecordingSource);
    const [id] = (await browser.finish(source.add([new File(['v'], NAME)]), routes)).ids;
    const stats = TestBed.inject(StatsFiles);
    await browser.finish(stats.pairFile(id, new File([STATS], STATS_NAME)), routes);
    await expect(browser.finish(logs.add(id, logStartingAt(0)), routes)).rejects.toThrow(
      /does not cover this run/,
    );
  });
});
