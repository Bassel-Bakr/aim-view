import { HttpRequest } from '@angular/common/http';
import { ResourceRef } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { ApiRoutes } from '../fake-api';
import { MODE_CASES, ModeCase, setUp } from './contract-case';
import { PastRun, ScoreHistory } from './score-history';
import { StatsFiles } from './stats-files';

/** A stats file: its name in KovaaK's stats folder, and its footer. */
interface FakeStats {
  name: string;
  text: string;
}

const footer = (score: string, kills: number, hits: number, misses: number) =>
  `Kill #,Timestamp\n1,16:22:20.833\n\nKills:,${kills}\nHit Count:,${hits}\nMiss Count:,${misses}\n` +
  `Score:,${score}\nScenario:,Air\n`;

const FILES: FakeStats[] = [
  { name: 'Air - Challenge - 2026.10.02-09.00.00 Stats.csv', text: footer('620.5', 60, 60, 20) },
  { name: 'Air - Challenge - 2026.10.01-16.23.04 Stats.csv', text: footer('558', 50, 50, 0) },
  { name: 'Air - Challenge - 2026.10.03-10.00.00 Stats.csv', text: 'Kills:,3\nScenario:,Air\n' },
  { name: 'Other - Challenge - 2026.10.01-16.00.00 Stats.csv', text: footer('1', 1, 1, 1) },
];

/** The runs of Air the stats files above hold, oldest first (the file without a score left out). */
const AIR: PastRun[] = [
  { stamp: '2026.10.01-16.23.04', score: 558, kills: 50, accuracy: 1 },
  { stamp: '2026.10.02-09.00.00', score: 620.5, kills: 60, accuracy: 0.75 },
];

/** A review server that knows the runs of Air only. */
const ROUTES: ApiRoutes = {
  '/api/history': (req: HttpRequest<unknown>) => (req.params.get('scenario') === 'Air' ? AIR : []),
};

/** A file as a folder input gives it: its path below the folder chosen. */
function chosen(f: FakeStats): File {
  const file = new File([f.text], f.name);
  Object.defineProperty(file, 'webkitRelativePath', { value: `stats/${f.name}` });
  return file;
}

/** The resource's value once it has one, while the fake server answers. */
async function settled(mode: ModeCase, ref: ResourceRef<PastRun[] | undefined>) {
  const read = (async () => {
    for (;;) {
      TestBed.tick();
      await new Promise((r) => setTimeout(r));
      if (ref.error()) throw ref.error();
      if (ref.hasValue() && !ref.isLoading()) return ref.value();
    }
  })();
  return mode.finish(read, ROUTES);
}

for (const mode of MODE_CASES) {
  describe(`ScoreHistory (${mode.name} mode)`, () => {
    it("gives a scenario's runs from its stats files, oldest first", async () => {
      const history = setUp(mode, ScoreHistory);
      const choose = TestBed.inject(StatsFiles).chooseFolder;
      if (choose) await choose(FILES.map(chosen));
      const ref = TestBed.runInInjectionContext(() => history.runs(() => 'Air'));
      const runs = await settled(mode, ref);
      expect(runs).toHaveLength(AIR.length);
      runs?.forEach((r, i) => {
        expect(r.stamp).toBe(AIR[i].stamp);
        expect(r.score).toBeCloseTo(AIR[i].score, 9);
        expect(r.kills).toBe(AIR[i].kills);
        expect(r.accuracy).toBeCloseTo(AIR[i].accuracy ?? Number.NaN, 9);
      });
    });

    it('gives none for a scenario without stats files', async () => {
      const history = setUp(mode, ScoreHistory);
      const ref = TestBed.runInInjectionContext(() => history.runs(() => 'Nowhere'));
      expect(await settled(mode, ref)).toEqual([]);
    });
  });
}
