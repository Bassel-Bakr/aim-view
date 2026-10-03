import { ClickSummary, ClickWhatIf, Flick, Issue } from '../../api';
import {
  clickWhatIf,
  directionRows,
  distanceRows,
  killStats,
  killsPerMinute,
  runHeadline,
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
      'Median TTK: 425 ms',
      'Confirmation: 100 ms',
      'Flick speed: 480 °/s',
      'Game FPS: 240',
      'Fastest next target: 80%',
      'Pathing in all: 1234 ms',
      'More shots with the best path: ≈ 2.0',
      'Accuracy: –',
      'Reaction: 80 ms',
      'Flick: 170 ms',
      'Click on the move: 20 °/s',
      'Click off center: –',
      'Kills a minute: –',
    ]);
  });

  it('counts the kills a minute from the first flick to the last kill', () => {
    const flicks = [
      { start_frame: 60, kill_frame: 90 },
      { start_frame: 100, kill_frame: 130 },
      { start_frame: 150, kill_frame: 660 },
    ] as Flick[];
    expect(killsPerMinute(flicks, 60)).toBe(18);
    expect(killsPerMinute(flicks.slice(0, 1), 60)).toBeNull();
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
      parts: [0.1, 0.2, 0.12, 0.08, 0.05],
    } as Flick;
    const s = killStats(m, SUMMARY, '+40 ms');
    expect(s[0].value).toBe('12.3°');
    expect(s[1].value).toBe('↑');
    expect(s[2]).toEqual({ label: 'TTK', value: '500 ms', detail: 'run 425 ms' });
    expect(s[5]).toMatchObject({ label: 'Flick landed', value: 'underflick 1.2°' });
    expect(s[10]).toMatchObject({ label: 'Pathing', value: '+40 ms' });
    expect(s[11]).toMatchObject({
      label: 'Micro',
      value: '200 ms',
      title: '120 ms onto the target, 80 ms settling',
    });
    expect(s).toHaveLength(16);
  });

  it('adds the forced reloads to the run and to each kill in a scenario whose magazine runs out', () => {
    const reloads = {
      ...SUMMARY,
      reloads: { count: 2, seconds: 1.6, score_lost: null },
    } as ClickSummary;
    // first flick to last kill: 60 s, so 1.6 s is 3% of the run
    const flicks = [
      { start_frame: 0, kill_frame: 30 },
      { start_frame: 40, kill_frame: 3600 },
    ] as Flick[];
    expect(runStats(SUMMARY, null)).toHaveLength(16);
    expect(runStats(reloads, null, flicks, 60)[16]).toMatchObject({
      label: 'Reloads',
      value: '2',
      detail: '1.60 s · 3% of the run',
    });
    const kill = {
      D0: 5,
      dir: 0,
      end_left: 0,
      past: 0,
      parts: null,
      reloads: 1,
      reload_time: 0.8,
    } as Flick;
    expect(killStats(kill, SUMMARY, '')).toHaveLength(16);
    expect(killStats(kill, reloads, '')[16]).toEqual({
      label: 'Reload',
      value: '800 ms',
      detail: 'forced by an empty magazine',
    });
    const clean = { ...kill, reloads: 0, reload_time: 0 };
    expect(killStats(clean, reloads, '')[16]).toMatchObject({ label: 'Reload', value: 'none' });
  });

  it('shows the path cards as under way while the tracks load', () => {
    expect(
      runStats(SUMMARY, null)
        .slice(7, 10)
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

  it('shows a run from the video alone (no score, shots, misses or accuracy) with dashes', () => {
    // python/review.py's summarize without a stats file: the meta has only the scenario
    const video = {
      scenario: 'Probe',
      score: null,
      kills: 40,
      misses: null,
      shots: null,
      accuracy: null,
      fps_avg: null,
      sens: null,
      radius: 0.45,
      measured: 38,
      median_interval: 0.5,
      info: { source: 'video', matched: 40, kills_stats: null },
    } as ClickSummary;
    const cards = runStats(video, { share: null, total: 0, extra: 0 });
    const tiles = runHeadline(video, []);
    const shown = [...cards, ...tiles].map((x) => `${x.label}: ${x.value}`);
    expect(shown).toContain('Score: –');
    expect(shown).toContain('Accuracy: –');
    expect(shown).toContain('Misses: –');
    expect(shown).toContain('More kills with the best path: ≈ 0.0');
    expect(tiles.find((t) => t.label === 'Kills')?.note).toBe('');
    const text = [...shown, ...tiles.map((t) => t.note), sourceNote(video)].join(' ');
    expect(text).not.toMatch(/NaN|undefined|null/);
    expect(sourceNote(video)).toContain('40 kills found in the video alone');
  });

  it('groups the what-if lines under Pace, Flicks and Micros, each biggest first', () => {
    const line = (group: ClickWhatIf['group'], what: string, kills: number): ClickWhatIf => ({
      group,
      what,
      kills,
      score: null,
      how: 'h',
    });
    const t = clickWhatIf([
      line('micros', 'Settle sooner', 6),
      line('flicks', 'Stop on the target', 4.25),
      line('pace', 'Start sooner', 3),
      line('flicks', 'Go straight there', 12.4),
    ]);
    expect(t.columns).toEqual(['Kills']);
    expect(t.groups.map((g) => g.name)).toEqual(['Pace', 'Flicks', 'Micros']);
    expect(t.groups[1].lines.map((l) => [l.what, ...l.gains])).toEqual([
      ['Go straight there', '+12 kills'],
      ['Stop on the target', '+4.3 kills'],
    ]);
  });

  it('adds the score column when a line knows its score, and leaves out empty groups', () => {
    const t = clickWhatIf([
      { group: 'flicks', what: 'a', kills: 2, score: 31.5, how: 'h' },
      { group: 'flicks', what: 'b', kills: 1, score: null, how: 'h' },
    ]);
    expect(t.columns).toEqual(['Kills', 'Score']);
    expect(t.groups.map((g) => g.name)).toEqual(['Flicks']);
    expect(t.groups[0].lines.map((l) => l.gains)).toEqual([
      ['+2.0 kills', '+32 score'],
      ['+1.0 kills', ''],
    ]);
  });

  it('shows no what-if lines on a report from an older core', () => {
    expect(clickWhatIf(undefined).groups).toEqual([]);
    expect(clickWhatIf([]).groups).toEqual([]);
  });
});
