import { ClickReport, Flick, TrackFrame } from '../../api';
import { analysePaths, pathing, pickText } from './path-analysis';

// Three targets: the first kill took the far one (at x 9) while a near one (x 2) was on screen; the second took the
// near one with nothing else left; the third's target appeared just before the flick, so it was new.
// (At 100 fps a target counts as new when it appeared under 15 frames before its flick began.)
const FLICKS = [
  { n: 1, start_frame: 30, kill_frame: 40, react: 0, D0: 9, total: 0.6 },
  { n: 2, start_frame: 50, kill_frame: 60, react: 0, D0: 2, total: 0.3 },
  { n: 3, start_frame: 70, kill_frame: 80, react: 0, D0: 4, total: 0.4 },
] as Flick[];

const frames: TrackFrame[] = Array.from({ length: 81 }, (_, i) => ({
  i,
  shift: [0, 0],
  t: [],
  a: [],
}));
frames[33].t = [
  [1, 9, 0],
  [2, 2, 0],
];
frames[40].t = [[1, 0, 0]];
frames[53].t = [[2, 2, 0]];
frames[60].t = [[2, 0, 0]];
frames[73].t = [[3, 4, 0]];
frames[80].t = [[3, 0, 0]];

const REPORT = {
  mode: 'click',
  fps: 100,
  flicks: FLICKS,
  paths: { '1': [[40, 0, 0]], '2': [[60, 0, 0]], '3': [[80, 0, 0]] },
  appeared: { '1': 0, '2': 0, '3': 68 },
  summary: { radius: 0.5, median_interval: 0.4, kills: 3, shots: 3, info: { source: 'stats' } },
} as unknown as ClickReport;

describe('analysePaths', () => {
  const a = analysePaths(REPORT, { fps: 100, frames });

  it('prices a pick of the far target over the near one', () => {
    expect(pickText(a, 1)).toMatch(/^\+\d+ ms$/);
    expect(a?.picks.get(1)?.best).toBe(false);
  });

  it('says when there was only one target, or the target was new', () => {
    expect(pickText(a, 2)).toBe('only one');
    expect(pickText(a, 3)).toBe('spawn');
  });

  it('flags the costly picks in the Pathing check, costliest first', () => {
    const p = pathing(a, REPORT);
    expect(p?.issue.value).toContain('The fastest next target in 0% of 1 picks');
    expect(p?.costliest.map((c) => c.flick.n)).toEqual([1]);
  });

  it('shows the work is under way until the tracks are in', () => {
    expect(pickText(null, 1)).toBe('…');
  });
});
