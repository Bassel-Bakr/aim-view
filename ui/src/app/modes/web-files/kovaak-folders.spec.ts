import { TestBed } from '@angular/core/testing';
import { ScenarioInfo } from '../../api';
import { CoreModule } from '../wasm/core-module';
import { findFolders } from './kovaak-folders';
import { ScenarioFacts, ScenarioSource, windowsOrder } from './scenario-facts';

/** A folder tree for tests: folders hold folders (objects) and files (strings, their text). */
interface Tree {
  [name: string]: Tree | string;
}

/** A stand-in for a folder's handle over a tree. */
function dir(name: string, tree: Tree): FileSystemDirectoryHandle {
  const handle = {
    kind: 'directory',
    name,
    async getDirectoryHandle(child: string) {
      const t = tree[child];
      if (typeof t !== 'object') throw new DOMException('not found', 'NotFoundError');
      return dir(child, t);
    },
    async *entries() {
      for (const [n, t] of Object.entries(tree))
        yield [
          n,
          typeof t === 'object'
            ? dir(n, t)
            : { kind: 'file', name: n, getFile: async () => new File([t], n) },
        ];
    },
  };
  return handle as unknown as FileSystemDirectoryHandle;
}

const STEAMAPPS: Tree = {
  common: {
    FPSAimTrainer: { FPSAimTrainer: { stats: {}, Saved: { SaveGames: { Scenarios: {} } } } },
  },
  workshop: { content: { '824270': {} } },
};

describe('findFolders', () => {
  it('finds the stats, scenarios and workshop folders below steamapps', async () => {
    const found = await findFolders(dir('steamapps', STEAMAPPS));
    expect(Object.keys(found).sort()).toEqual(['scenarios', 'stats', 'workshop']);
    expect(found.workshop?.name).toBe('824270');
  });

  it('takes a picked folder for what its name says, and finds what the game folder holds', async () => {
    expect(Object.keys(await findFolders(dir('stats', {})))).toEqual(['stats']);
    const game = await findFolders(
      dir('FPSAimTrainer', {
        FPSAimTrainer: { stats: {}, Saved: { SaveGames: { Scenarios: {} } } },
      }),
    );
    expect(Object.keys(game).sort()).toEqual(['scenarios', 'stats']);
  });
});

describe('ScenarioFacts', () => {
  /** A core stand-in: the kind is the text, so the test sees which file's facts won. */
  const parse = async (text: string): Promise<ScenarioInfo> => ({
    kind: text.trim() as ScenarioInfo['kind'],
    limit: 60,
    targets: 3,
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
