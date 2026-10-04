import { TestBed } from '@angular/core/testing';
import { ScenarioInfo } from '../../api';
import { CoreModule } from '../wasm/core-module';
import { BrowserStore } from './browser-store';
import { sortChosen } from './kovaak-folders';
import { ScenarioFacts, ScenarioSource, windowsOrder } from './scenario-facts';

describe('sortChosen', () => {
  /** A file as a folder input gives it: its path below the folder chosen. */
  const chosen = (path: string): File => {
    const f = new File(['x'], path.slice(path.lastIndexOf('/') + 1));
    Object.defineProperty(f, 'webkitRelativePath', { value: path });
    return f;
  };

  it("sorts a chosen folder's files into stats files and scenario files, in Python's reading order", () => {
    const out = sortChosen([
      chosen('steamapps/workshop/content/824270/9/Air.sce'),
      chosen(
        'steamapps/common/FPSAimTrainer/FPSAimTrainer/stats/Air - Challenge - 2026.10.01-16.23.04 Stats.csv',
      ),
      chosen('steamapps/workshop/content/824270/10/b.sce'),
      chosen('steamapps/common/FPSAimTrainer/FPSAimTrainer/Saved/SaveGames/Scenarios/Air.sce'),
      chosen('steamapps/common/FPSAimTrainer/FPSAimTrainer/Content/Paks/game.pak'),
      chosen('steamapps/common/FPSAimTrainer/FPSAimTrainer/Saved/Config/stats.csv'),
    ]);
    expect(out.stats.map((f) => f.name)).toEqual([
      'Air - Challenge - 2026.10.01-16.23.04 Stats.csv',
    ]);
    expect(out.scenarios.map((s) => s.path)).toEqual([
      'scenarios/Air.sce',
      'workshop/10/b.sce',
      'workshop/9/Air.sce',
    ]);
  });

  it('takes a folder chosen by itself for what its name says', () => {
    expect(sortChosen([chosen('stats/a.csv')]).stats.length).toBe(1);
    expect(sortChosen([chosen('Scenarios/a.sce')]).scenarios[0].path).toBe('scenarios/a.sce');
    expect(sortChosen([chosen('824270/5/a.sce')]).scenarios[0].path).toBe('workshop/5/a.sce');
  });
});

describe('ScenarioFacts', () => {
  /** A core stand-in: the kind is the text, so the test sees which file's facts won. */
  const parse = async (text: string): Promise<ScenarioInfo> => ({
    kind: text.trim() as ScenarioInfo['kind'],
    limit: 60,
    targets: 3,
    reload: null,
  });
  const source = (path: string, name: string, text: string): ScenarioSource => ({
    path,
    name,
    file: async () => new File([text + '\n[Map Data]\nmore'], name),
  });

  it("keeps the last file's facts for a name, as Python's dict does, and reads only the part before the map", async () => {
    let read = '';
    TestBed.configureTestingModule({
      providers: [
        { provide: CoreModule, useValue: { scenarioFacts: (t: string) => ((read = t), parse(t)) } },
      ],
    });
    const facts = TestBed.inject(ScenarioFacts);
    await facts.read([
      source('scenarios/Air.sce', 'Air.sce', 'static'),
      source('workshop/1/Air.sce', 'Air.sce', 'tracking'),
    ]);
    expect(facts.kind('AIR')).toBe('tracking');
    expect(facts.get('nothing')).toBeNull();
    expect(read).toBe('tracking\n');
  });

  it('keeps the facts for the next visit, and a folder read again replaces only its own files', async () => {
    const kept = new Map<string, unknown>();
    const store = {
      get: async (k: string) => kept.get(k),
      set: async (k: string, v: unknown) => void kept.set(k, v),
    };
    const setUp = () => {
      TestBed.resetTestingModule();
      TestBed.configureTestingModule({
        providers: [
          { provide: CoreModule, useValue: { scenarioFacts: parse } },
          { provide: BrowserStore, useValue: store },
        ],
      });
      return TestBed.inject(ScenarioFacts);
    };
    const facts = setUp();
    await facts.read([source('scenarios/Air.sce', 'Air.sce', 'static')]);
    await facts.read([source('workshop/1/Bot.sce', 'Bot.sce', 'tracking')]);
    expect(facts.kind('air')).toBe('static');
    expect(facts.kind('bot')).toBe('tracking');
    const next = setUp();
    await new Promise((r) => setTimeout(r));
    expect(next.kind('air')).toBe('static');
    expect([...next.sources()]).toEqual(['scenarios', 'workshop']);
  });

  it('orders names as Windows lists them', () => {
    expect(['b.sce', 'A.sce', 'a_.sce', '10', '9'].sort(windowsOrder)).toEqual([
      '10',
      '9',
      'A.sce',
      'a_.sce',
      'b.sce',
    ]);
  });
});
