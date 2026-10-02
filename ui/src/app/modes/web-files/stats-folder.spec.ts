import { TestBed } from '@angular/core/testing';
import { LocalFiles } from './local-files';
import { StatsFolder } from './stats-folder';

const STATS = 'Kill #,Timestamp\n1,16:22:20.833\n\nKills:,9\nScore:,558\nScenario:,Air\n';
const NAMES = [
  'Air - Challenge - 2026.10.01-16.23.04 Stats.csv',
  'Air - Challenge - 2026.10.01-16.21.38 Stats.csv',
  'Air Tracking - Challenge - 2026.10.01-16.23.05 Stats.csv',
  'notes.txt',
];

describe('StatsFolder', () => {
  it('indexes the stats files by scenario, and finds a run within five seconds of its time', () => {
    const folder = TestBed.inject(StatsFolder);
    folder.index(NAMES);
    expect(folder.state()).toBe('ready');
    expect(folder.files).toBe(3);
    const t = Date.UTC(2026, 9, 1, 16, 23, 3) / 1000;
    expect(folder.find('Air', t)?.name).toBe(NAMES[0]);
    expect(folder.find('Air', t + 10)).toBeNull();
    expect(folder.find('air', t)).toBeNull();
  });

  it("offers the scenario's files nearest the time first, or any scenario whose name holds the search", () => {
    const folder = TestBed.inject(StatsFolder);
    folder.index(NAMES);
    const t = Date.UTC(2026, 9, 1, 16, 23, 3) / 1000;
    expect(folder.candidates('air', null, t).map((c) => [c.name, c.off])).toEqual([
      [NAMES[0], 1],
      [NAMES[1], -85],
    ]);
    expect(folder.candidates('Air', 'track', t).map((c) => c.scenario)).toEqual(['Air Tracking']);
  });

  it('pairs an added recording with its stats file by name and time once the folder is open', async () => {
    const folder = TestBed.inject(StatsFolder);
    const local = TestBed.inject(LocalFiles);
    const [id] = (await local.add([new File(['v'], 'Air - 558 - 2026.10.01-16.23.03.mp4')])).ids;
    expect(local.find(id)?.statsHow).toBe('missing');
    await folder.openFiles([new File([STATS], NAMES[0])]);
    await local.findAllStats();
    expect(local.find(id)?.statsHow).toBe('found');
    expect(local.recordings()[0].stats).toBe(true);
    local.unpair(id);
    await local.findAllStats();
    expect(local.find(id)?.statsHow).toBe('none');
  });
});
