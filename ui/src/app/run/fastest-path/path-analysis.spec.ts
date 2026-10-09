import { ClickReport, Flick, TrackFrame, TrackPoint, Tracks } from '../../api';
import { fingerprint } from '../recording-context';
import { analysePaths, extraShots, pathing, pickText } from './path-analysis';

// Three targets: the first kill took the far one (at x 9) while a near one (x 2) was on screen; the second took the
// near one with nothing else left; the third's target appeared just before the flick, so it was new.
// (At 100 fps a target counts as new when it appeared under 15 frames before its flick began.)
const FLICKS = [
  { kill_number: 1, start_frame: 30, kill_frame: 40, react: 0, D0: 9, total: 0.6 },
  { kill_number: 2, start_frame: 50, kill_frame: 60, react: 0, D0: 2, total: 0.3 },
  { kill_number: 3, start_frame: 70, kill_frame: 80, react: 0, D0: 4, total: 0.4 },
] as Flick[];

/** The targets on screen at the frames that have any: each its id and place in degrees. */
const TARGETS_AT: Record<number, TrackPoint[]> = {
  33: [
    [1, 9, 0],
    [2, 2, 0],
  ],
  40: [[1, 0, 0]],
  53: [[2, 2, 0]],
  60: [[2, 0, 0]],
  73: [[3, 4, 0]],
  80: [[3, 0, 0]],
};

/** A frame of tracks with the given targets. */
function trackFrame(index: number, targets: TrackPoint[]): TrackFrame {
  // eslint-disable-next-line id-length -- the core names TrackFrame's fields (generated/track-frame.ts)
  return { i: index, shift: [0, 0], t: targets, a: [] };
}

const frames = Array.from({ length: 81 }, (_unused, i) => trackFrame(i, TARGETS_AT[i] ?? []));

const REPORT = {
  mode: 'click',
  fps: 100,
  flicks: FLICKS,
  paths: { '1': [[40, 0, 0]], '2': [[60, 0, 0]], '3': [[80, 0, 0]] },
  appeared: { '1': 0, '2': 0, '3': 68 },
  summary: { radius: 0.5, median_interval: 0.4, kills: 3, shots: 3, info: { source: 'stats' } },
} as unknown as ClickReport;

describe('analysePaths', () => {
  const analysis = analysePaths(REPORT, { fps: 100, frames });

  it('prices a pick of the far target over the near one', () => {
    expect(pickText(analysis, 1)).toMatch(/^\+\d+ ms$/);
    expect(analysis?.picks.get(1)?.best).toBe(false);
  });

  it('says when there was only one target, or the target was new', () => {
    expect(pickText(analysis, 2)).toBe('only one');
    expect(pickText(analysis, 3)).toBe('spawn');
  });

  it('flags the costly picks in the Pathing check, costliest first', () => {
    const check = pathing(analysis, REPORT);
    expect(check?.issue.value).toContain('The fastest next target in 0% of 1 picks');
    expect(check?.costliest.map((costly) => costly.flick.kill_number)).toEqual([1]);
  });

  it('shows the work is under way until the tracks are in', () => {
    expect(pickText(null, 1)).toBe('…');
  });

  it('keeps every pick, its cost and the check (a digest of them)', () => {
    const digests = [variedRun('stats'), variedRun('hud')].map(([report, tracks]) => {
      const analysis = analysePaths(report, tracks);
      if (!analysis) return null;
      const outcome = [
        [...analysis.picks.entries()],
        [...analysis.killOf.entries()].map(([id, kill]) => [id, kill.kill_number]),
        [...analysis.firstSeen.entries()],
        [analysis.total, analysis.share, analysis.firstStart, analysis.lastKill],
        pathing(analysis, report),
        report.flicks.map((kill) => pickText(analysis, kill.kill_number)),
        extraShots(analysis, report, 1.5),
      ];
      return fingerprint(JSON.stringify(outcome));
    });
    expect(digests).toEqual(['dd647761', 'ace073f5']);
  });
});

/** A target's place in degrees. */
type Place = [x: number, y: number];

/** Where a target sits: each its own place. */
function placeOf(id: number): Place {
  return [((id * 37) % 19) - 9, ((id * 23) % 11) - 5];
}

/** A run's report and its tracks. */
type VariedRun = [report: ClickReport, tracks: Tracks];

/**
 * Twelve kills at 60 fps, four targets on screen at a time: kill k takes target k at frame 30k + 20, and target k + 4
 * appears as it dies. Without a stats file the run starts two median kills before the first kill, and the user's mark
 * sets its end.
 */
function variedRun(source: string): VariedRun {
  const killFrame = (id: number) => 30 * id + 20;
  const appearFrame = (id: number) => Math.max(0, killFrame(id - 4));
  const frames: TrackFrame[] = Array.from({ length: 420 }, (_unused, i) => {
    const targets = Array.from({ length: 16 }, (_unusedToo, j) => j + 1)
      .filter((id) => appearFrame(id) <= i && i <= killFrame(id))
      .map((id): TrackPoint => [id, ...placeOf(id)]);
    return trackFrame(i, targets);
  });
  const flicks = Array.from({ length: 12 }, (_unused, i) => {
    const id = i + 1;
    const [x, y] = placeOf(id);
    return {
      kill_number: id,
      start_frame: killFrame(id) - 12,
      kill_frame: killFrame(id),
      react: 0.05,
      D0: Math.hypot(x, y),
      total: 0.25 + (id % 3) / 20,
    } as Flick;
  });
  const paths = Object.fromEntries(
    flicks.map((kill) => [
      String(kill.kill_number),
      [[kill.kill_frame, ...placeOf(kill.kill_number)]],
    ]),
  );
  const report = {
    mode: 'click',
    fps: 60,
    flicks,
    paths,
    run: source === 'stats' ? undefined : { start: null, end: 5.5 },
    summary: {
      radius: 0.5,
      median_interval: 0.3,
      kills: 12,
      shots: source === 'stats' ? 14 : null,
      info: { source },
    },
  } as unknown as ClickReport;
  return [report, { fps: 60, frames }];
}
