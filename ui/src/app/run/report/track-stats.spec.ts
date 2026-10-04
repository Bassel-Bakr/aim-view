import { Motion, MotionBand, TrackSummary, WhatIf } from '../../api';
import { fingerprint } from '../recording-context';
import { motionView, trackNote, trackStats, whatIfTable } from './track-stats';

const SUMMARY = {
  score: 13278,
  on_target: 0.79,
  on_all: 0.75,
  accuracy: 0.74,
  error: 0.48,
  lost: 0.92,
  lost_cost: 0.16,
  slip_cost: 0.03,
  back: 0.167,
  longest_off: 1.25,
  bots: 0,
  fps_avg: 240,
  faint: null,
} as TrackSummary;

describe('track stats', () => {
  it('shows the time on the bot and the drops off it, each with why it matters', () => {
    const stats = trackStats(SUMMARY);
    expect(stats.map((x) => x.label)).not.toContain('Bots killed');
    const lost = stats.find((x) => x.label === 'Lost the bot, per second');
    expect(lost).toMatchObject({ value: '0.92', detail: 'cost 16% accuracy' });
    expect(lost?.why).toContain('slips shorter than 0.1 s cost 3% more');
    expect(stats.find((x) => x.label === 'Longest off')?.value).toBe('1.25 s');
    expect(stats.find((x) => x.label === 'Time to get back')?.value).toBe('167 ms');
  });

  it('adds the switching between bots when bots die', () => {
    const stats = trackStats({ ...SUMMARY, bots: 12, switching: 0.08 });
    expect(stats.map((x) => x.label)).toContain('On target, whole run');
    expect(stats.find((x) => x.label === 'Switching')?.value).toBe('8%');
    expect(trackNote({ ...SUMMARY, bots: 12 })).toContain('Bots die here');
  });

  it('says what the faint-target cut-off left out', () => {
    expect(trackNote({ ...SUMMARY, faint: { offset: 0.3, cut: 0.565, tracks: 13 } })).toContain(
      '13 tracks scoring under 0.565 are left out',
    );
    expect(trackNote({ ...SUMMARY, faint: { offset: 0.3, cut: null, tracks: 0 } })).toContain(
      'no detector scores',
    );
  });
});

describe('track stats', () => {
  it('says why the following was not measured', () => {
    const view = motionView({
      reason: 'the bot barely moved',
      seconds: 3.2,
      camera: 0.5,
    } as Motion);
    expect(view?.reason).toBe(
      "Not measured: the bot barely moved (3.2 s); the camera's turn was read in 50% of the frames.",
    );
  });

  it('shows the lag as behind or ahead', () => {
    const view = motionView({
      camera: 1,
      lag: -0.16,
      lag_ms: -9.1,
      by_direction: [] as MotionBand[],
    } as Motion);
    expect(view?.stats[0]).toMatchObject({ value: '0.16° behind', detail: '9 ms at its speed' });
  });

  it('shows each change as a gain in accuracy', () => {
    expect(
      whatIfTable([{ what: "Don't trail", gain: 0.075, how: 'x' }]).groups[0].lines[0].gains,
    ).toEqual(['+7.5%']);
    expect(whatIfTable([]).groups).toEqual([]);
  });

  it('shows a run without a stats file (no score or accuracy) with dashes', () => {
    const stats = trackStats({ ...SUMMARY, score: null, accuracy: null, fps_avg: null });
    const shown = stats.map((x) => `${x.label}: ${x.value}`);
    expect(shown).toContain('Score: –');
    expect(shown).toContain('Accuracy (stats file): –');
    expect(shown).toContain('Game FPS: –');
    expect(shown.join(' ')).not.toMatch(/NaN|undefined|null/);
  });
});

/** A followed bot, every measure of it found. */
const MOTION = {
  camera: 0.93,
  seconds: 41.3,
  lag: -0.12,
  lag_ms: -35.4,
  off_behind: 0.4,
  off_ahead: 0.35,
  off_side: 0.25,
  overshoots: 0.7,
  overshoot_dist: 0.3,
  overcorrect: 0.2,
  corrections: 40,
  swing_count: 8,
  reaction: 183.2,
  reversals: 12,
  reversal_overshoot: 0.33,
  reversal_overshoot_dist: 0.25,
  error_h: 0.3,
  error_v: 0.1,
  target_speed: 35.5,
  by_direction: [
    { name: 'right', share: 0.5, on: 0.7, distance: 0.3, lag: -0.1 },
    { name: 'left', share: 0.5, on: 0.6, distance: 0.4, lag: 0.2 },
  ],
} as unknown as Motion;

describe('track stats', () => {
  it('keeps every card, row and note (a digest of them)', () => {
    const withBots = {
      ...SUMMARY,
      bots: 3,
      to_next: 0.5,
      waiting: 0.2,
      onto: 0.3,
      switching: 0.12,
      faint: { cut: 0.4, tracks: 3 },
    } as TrackSummary;
    const plain = {
      ...SUMMARY,
      lost_cost: null,
      slip_cost: null,
      lost: null,
      fps_avg: null,
    } as unknown as TrackSummary;
    const outputs = [
      [trackStats(SUMMARY), trackStats(withBots), trackStats(plain)],
      [
        trackNote(SUMMARY),
        trackNote(withBots),
        trackNote({ ...SUMMARY, faint: { cut: null, tracks: 0 } } as TrackSummary),
      ],
      [motionView(MOTION), motionView({ camera: 0.5 } as Motion), motionView(null)],
      whatIfTable([{ what: 'Stay on longer', gain: 0.031, how: 'by tracking' }] as WhatIf[]),
    ];
    expect(outputs.map((output) => fingerprint(JSON.stringify(output)))).toEqual([
      'da621f67',
      '9f200b5a',
      'd30984f6',
      '39cb5229',
    ]);
  });
});
