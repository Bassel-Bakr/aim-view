import { HttpRequest } from '@angular/common/http';
import { TestBed } from '@angular/core/testing';
import { ApiRoutes } from '../fake-api';
import { mouseRun } from '../fake-mouse';
import { CoreModule } from '../modes/wasm/core-module';
import { logUtcOffset } from '../modes/wasm/browser-mouse-logs';
import { MouseReadOutcome, MouseReadRequest } from '../mouse-api';
import { MODE_CASES, ModeCase } from './contract-case';
import { MouseLogs } from './mouse-logs';
import { RecordingSource } from './recording-source';
import { StatsFiles } from './stats-files';

const NAME = 'Air - 1 - 2026.10.01-16.23.03.mp4';
const STATS_NAME = 'Air - Challenge - 2026.10.01-16.23.03 Stats.csv';
const STATS = 'Kill #,Timestamp\n1,16:22:20.833\n\nKills:,1\nScore:,1\nScenario:,Air\n';

/** A review server that takes the recording added (the server mode reads no logs). */
const ROUTES: ApiRoutes = {
  '/api/vods': [],
  '/api/upload': (req: HttpRequest<unknown>) => {
    const name = req.params.get('name') ?? '';
    return { id: `uploads/${name}`, saved: name };
  },
};

/** The core's mouse reader, standing in: it answers with `outcome` and keeps what it was asked. */
class StandInCore {
  asked: MouseReadRequest[] = [];
  constructor(private readonly outcome: MouseReadOutcome) {}

  mouseRead(_log: Uint8Array, request: string): Promise<string> {
    this.asked.push(JSON.parse(request) as MouseReadRequest);
    return Promise.resolve(JSON.stringify(this.outcome));
  }
}

function setUp(mode: ModeCase, core: StandInCore): MouseLogs {
  TestBed.configureTestingModule({
    providers: [...mode.providers(), { provide: CoreModule, useValue: core }],
  });
  return TestBed.inject(MouseLogs);
}

async function settled(): Promise<void> {
  for (let i = 0; i < 4; i++) {
    TestBed.tick();
    await new Promise((r) => setTimeout(r));
  }
}

/** A log's header and nothing else: it started at the given time. */
function logStartingAt(ms: number): File {
  const head = new Uint8Array(32);
  new DataView(head.buffer).setBigInt64(24, BigInt(ms) * 1_000_000n, true);
  return new File([head], 'mouse_2026-10-01_16-22-00.bin');
}

for (const mode of MODE_CASES) {
  describe(`MouseLogs (${mode.name} mode)`, () => {
    it('measures a recording that has no log as none, and logs nothing itself', async () => {
      const logs = setUp(mode, new StandInCore({ error: 'not asked' }));
      const id = (
        await mode.finish(TestBed.inject(RecordingSource).add([new File(['v'], NAME)]), ROUTES)
      ).ids[0];
      const measures = TestBed.runInInjectionContext(() => logs.measures(() => id));
      await settled();
      expect(measures.value() ?? null).toBeNull();
      expect(logs.logs).toBe(false);
    });

    it(
      mode.name === 'browser' ? 'reads a log added for the run, and keeps it' : 'turns down a log',
      async () => {
        const core = new StandInCore({ run: mouseRun() });
        const logs = setUp(mode, core);
        const id = (
          await mode.finish(TestBed.inject(RecordingSource).add([new File(['v'], NAME)]), ROUTES)
        ).ids[0];
        const log = logStartingAt(Date.UTC(2026, 9, 1, 13, 22));
        if (!logs.adds) {
          await expect(logs.add(id, log)).rejects.toThrow(/no mouse logs/);
          return;
        }
        await expect(logs.add(id, log)).rejects.toThrow(/no stats file/);
        await TestBed.inject(StatsFiles).pairFile(id, new File([STATS], STATS_NAME));
        const measured = await logs.add(id, log);
        expect(measured.run?.matched).toBe(2);
        expect(core.asked.at(-1)).toEqual({
          stats_name: STATS_NAME,
          stats_text: STATS,
          utc_offset: -new Date(Date.UTC(2026, 9, 1, 13, 22)).getTimezoneOffset() * 60,
        });
        const measures = TestBed.runInInjectionContext(() => logs.measures(() => id));
        await settled();
        expect(measures.value()?.file).toBe('mouse_2026-10-01_16-22-00.bin');
        await logs.forget(id);
        await settled();
        expect(measures.value() ?? null).toBeNull();
      },
    );
  });
}

describe('MouseLogs: what the browser turns down', () => {
  it('keeps no log that does not cover the run, and says why', async () => {
    const [browser] = MODE_CASES;
    const logs = setUp(
      browser,
      new StandInCore({ error: 'the log (16:00 to 16:01) does not cover this run' }),
    );
    const [id] = (await TestBed.inject(RecordingSource).add([new File(['v'], NAME)])).ids;
    await TestBed.inject(StatsFiles).pairFile(id, new File([STATS], STATS_NAME));
    await expect(logs.add(id, logStartingAt(0))).rejects.toThrow(/does not cover this run/);
    const measures = TestBed.runInInjectionContext(() => logs.measures(() => id));
    await settled();
    expect(measures.value() ?? null).toBeNull();
  });

  it("takes the time zone at the log's start", () => {
    const at = Date.UTC(2026, 0, 15, 12);
    const head = new Uint8Array(32);
    new DataView(head.buffer).setBigInt64(24, BigInt(at) * 1_000_000n, true);
    expect(logUtcOffset(head)).toBe(-new Date(at).getTimezoneOffset() * 60);
    expect(logUtcOffset(new Uint8Array(8))).toBe(0);
  });
});
