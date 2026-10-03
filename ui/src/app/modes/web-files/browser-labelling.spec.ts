import { recording } from '../../fake-api';
import { labelQueue } from './browser-labelling';

const none = new Set<string>();

describe('labelQueue', () => {
  it('puts added files first, then the newest recording of each scenario folder', () => {
    const list = [
      recording({ id: 'folder:Air/new.mp4', mtime: 30 }),
      recording({ id: 'folder:Air/old.mp4', mtime: 20 }),
      recording({ id: 'folder:Bounce/b.mp4', mtime: 10 }),
      recording({ id: 'local:1/theirs.mp4', mtime: 5 }),
      recording({ id: 'local:2/another.mp4', mtime: 1 }),
    ];
    expect(labelQueue(list, none, none)).toEqual([
      'local:1/theirs.mp4',
      'local:2/another.mp4',
      'folder:Air/new.mp4',
      'folder:Bounce/b.mp4',
    ]);
  });

  it('leaves out probes, other games, skipped ones and those with saved areas', () => {
    const list = [
      recording({ id: 'folder:Probe Static/p.mp4', mtime: 50 }),
      recording({ id: 'folder:Air/other-game.mp4', mtime: 40, not_aim: true }),
      recording({ id: 'folder:Air/skipped.mp4', mtime: 30 }),
      recording({ id: 'folder:Air/saved.mp4', mtime: 20 }),
      recording({ id: 'folder:Air/left.mp4', mtime: 10 }),
    ];
    const skipped = new Set(['Air/skipped.mp4']);
    const labelled = new Set(['Air/saved.mp4']);
    expect(labelQueue(list, skipped, labelled)).toEqual(['folder:Air/left.mp4']);
  });

  it('groups the videos at the top of the folder opened by their scenario', () => {
    const list = [
      recording({ id: 'folder:a.mp4', scenario: 'Air', mtime: 3 }),
      recording({ id: 'folder:b.mp4', scenario: 'Air', mtime: 2 }),
      recording({ id: 'folder:c.mp4', scenario: 'Bounce', mtime: 1 }),
    ];
    expect(labelQueue(list, none, none)).toEqual(['folder:a.mp4', 'folder:c.mp4']);
  });
});
