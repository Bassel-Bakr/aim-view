import { ClickSummary, Flick, Issue } from '../../api';
import {
  directionRows,
  distanceRows,
  killStats,
  runStats,
  sortedIssues,
  sourceNote,
} from './click-stats';

const SUMMARY = {
  score: 889.26,
  kills: 98,
  misses: 3,
  shots: 101,
  median_interval: 0.425,
  still: 0.1,
  peak: 480.4,
  fps_avg: 239.6,
  react: 0.08,
  flick: 0.17,
  click_speed: 20,
  radius: 0.45,
  measured: 96,
  sens: '1.5 cm/360',
  info: { source: 'stats', matched: 98, kills_stats: 98 },
} as ClickSummary;

describe('click stats', () => {
  it("shows the whole run's numbers", () => {
    const s = runStats(SUMMARY, { share: 0.8, total: 1.234, extra: 2.04 });
    expect(s.map((x) => `${x.label}: ${x.value}`)).toEqual([
      'Score: 889.26',
      'Kills: 98',
      'Misses: 3',
      'Median kill: 425 ms',
      'Still before the click: 100 ms',
      'Peak speed: 480 °/s',
      'Game FPS: 240',
      'Fastest next target: 80%',
      'Path cost in all: 1234 ms',
      'More shots with the best path: ≈ 2.0',
    ]);
  });

  it("shows a kill's numbers with the run's medians under them", () => {
    const m = {
      D0: 12.34,
      dir: 90,
      total: 0.5,
      react: 0.1,
      flick: 0.2,
      end_left: 1.2,
      still: 0.05,
      peak: 500,
      click_speed: 30,
      shots: 2,
    } as Flick;
    const s = killStats(m, SUMMARY, '+40 ms');
    expect(s[0].value).toBe('12.3° ↑');
    expect(s[1]).toEqual({ label: 'Kill time', value: '500 ms', detail: 'run 425 ms' });
    expect(s[4].value).toBe('short, 1.2° to go');
    expect(s.at(-1)).toMatchObject({ label: 'Path cost', value: '+40 ms' });
  });

  it('shows the path cards as under way while the tracks load', () => {
    expect(
      runStats(SUMMARY, null)
        .slice(-3)
        .map((x) => x.value),
    ).toEqual(['…', '…', '…']);
  });

  it('says where the kills came from and what was measured', () => {
    expect(sourceNote(SUMMARY)).toBe(
      '98 of 98 kills matched with the stats file · 96 flicks measured · target radius 0.45° · 1.5 cm/360',
    );
  });

  it('puts the checks to look at first', () => {
    const issues = [
      { title: 'a', flag: 'fine' },
      { title: 'b', flag: 'attention' },
    ] as Issue[];
    expect(sortedIssues(issues).map((i) => i.title)).toEqual(['b', 'a']);
  });

  it('labels the bands and the time beyond the distance', () => {
    expect(
      distanceRows([
        { lo: 60, hi: 90, n: 2, interval: 1, react: 0.1, short: 0.5, past: 0, still: 0.2 },
      ])[0].band,
    ).toBe('60° and over');
    const [d] = directionRows([
      { name: 'up-left', n: 3, interval: 0.4, distance: 9.87, short: 0, past: 0, beyond: -0.012 },
    ]);
    expect(d).toMatchObject({ toward: '↖ up-left', distance: '9.9°', beyond: '−12 ms' });
  });
});
