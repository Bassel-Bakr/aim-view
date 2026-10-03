import { Motion, TrackSummary } from '../../api';
import { motionView, trackNote, trackStats, whatIfRows } from './track-stats';

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
    const s = trackStats(SUMMARY);
    expect(s.map((x) => x.label)).not.toContain('Bots killed');
    const lost = s.find((x) => x.label === 'Lost the bot, per second');
    expect(lost).toMatchObject({ value: '0.92', detail: 'cost 16% accuracy' });
    expect(lost?.why).toContain('slips shorter than 0.1 s cost 3% more');
    expect(s.find((x) => x.label === 'Longest off')?.value).toBe('1.25 s');
    expect(s.find((x) => x.label === 'Time to get back')?.value).toBe('167 ms');
  });

  it('adds the switching between bots when bots die', () => {
    const s = trackStats({ ...SUMMARY, bots: 12, switching: 0.08 });
    expect(s.map((x) => x.label)).toContain('On target, whole run');
    expect(s.find((x) => x.label === 'Switching')?.value).toBe('8%');
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

  it('says why the following was not measured', () => {
    const m = motionView({ reason: 'the bot barely moved', seconds: 3.2, camera: 0.5 } as Motion);
    expect(m?.reason).toBe(
      "Not measured: the bot barely moved (3.2 s); the camera's turn was read in 50% of the frames.",
    );
  });

  it('shows the lag as behind or ahead', () => {
    const m = motionView({ camera: 1, lag: -0.16, lag_ms: -9.1, by_direction: [] } as Motion);
    expect(m?.stats[0]).toMatchObject({ value: '0.16° behind', detail: '9 ms at its speed' });
  });

  it('shows each change as a gain in accuracy', () => {
    expect(whatIfRows([{ what: "Don't trail", gain: 0.075, how: 'x' }])[0].gain).toBe('+7.5%');
  });

  it('shows a run without a stats file (no score or accuracy) with dashes', () => {
    const s = trackStats({ ...SUMMARY, score: null, accuracy: null, fps_avg: null });
    const shown = s.map((x) => `${x.label}: ${x.value}`);
    expect(shown).toContain('Score: –');
    expect(shown).toContain('Accuracy (stats file): –');
    expect(shown).toContain('Game FPS: –');
    expect(shown.join(' ')).not.toMatch(/NaN|undefined|null/);
  });
});
