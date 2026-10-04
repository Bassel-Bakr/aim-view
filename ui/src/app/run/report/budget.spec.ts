import { ClickSummary, Flick, KillParts } from '../../api';
import { averageReload, budget } from './budget';

const AVERAGE: KillParts = [0.1, 0.2, 0.1, 0.05, 0.05];

describe('budget', () => {
  it("shows the average kill's four steps, named inside the bar where they fit", () => {
    const b = budget(AVERAGE, null);
    expect(b?.title).toBe("Where an average kill's 500 ms goes");
    expect(b?.bars[0].segments.map((segment) => segment.text)).toEqual([
      'Reaction 100 ms',
      'Flick 200 ms',
      'Micro 150 ms',
      '',
    ]);
    expect(b?.bars[0].segments[2].title).toBe(
      'Micro 150 ms: 100 ms onto the target, 50 ms settling',
    );
  });

  it('puts a picked kill over the average on one time scale', () => {
    const flick = { kill_number: 7, total: 1, parts: [0.2, 0.4, 0.2, 0.1, 0.1] } as Flick;
    const b = budget(AVERAGE, flick);
    expect(b?.title).toBe("Where kill 7's 1000 ms goes");
    expect(b?.bars.map((x) => x.width)).toEqual([100, 50]);
    expect(b?.bars[1].label).toBe('Average kill, 500 ms');
    expect(b?.legend[0]).toMatchObject({ text: 'Reaction 200 ms', average: '(avg 100 ms)' });
    expect(b?.legend[2]).toMatchObject({ text: 'Micro 300 ms', average: '(avg 150 ms)' });
  });

  it("says so when a kill's steps could not be split", () => {
    const b = budget(AVERAGE, { kill_number: 3, total: 0.8, parts: undefined } as Flick);
    expect(b?.note).toContain('was not found');
    expect(b?.bars).toHaveLength(1);
  });
});

describe('budget', () => {
  it('shows a forced reload as a fifth step, taken from the confirmation and then the micro', () => {
    // a 120 ms reload takes the whole 50 ms confirmation and 70 ms of the 150 ms micro: the TTK stays 500 ms
    const flick = {
      kill_number: 4,
      total: 0.5,
      parts: [0.1, 0.2, 0.1, 0.05, 0.05],
      reloads: 1,
      reload_time: 0.12,
    } as Flick;
    const b = budget(AVERAGE, flick, 0.01);
    const kill = b?.bars[0].segments ?? [];
    expect(kill.map((segment) => segment.title)).toEqual([
      'Reaction 100 ms',
      'Flick 200 ms',
      'Micro 80 ms: 100 ms onto the target, 50 ms settling, less 70 ms under the reload',
      'Confirmation 0 ms: 50 ms, less 50 ms under the reload',
      'Reload 120 ms (taken from the confirmation and micro it overlapped)',
    ]);
    expect(kill.reduce((sum, segment) => sum + segment.grow, 0)).toBeCloseTo(1, 12);
    expect(b?.legend.map((item) => item.text)).toEqual([
      'Reaction 100 ms',
      'Flick 200 ms',
      'Micro 80 ms',
      'Confirmation 0 ms',
      'Reload 120 ms',
    ]);
    // the average kill's 10 ms reload comes out of its 50 ms confirmation
    expect(b?.legend.slice(3).map((item) => item.average)).toEqual(['(avg 40 ms)', '(avg 10 ms)']);
  });

  it('shows no more of a reload than the confirmation and micro it overlapped', () => {
    const flick = {
      kill_number: 5,
      total: 0.5,
      parts: AVERAGE,
      reloads: 1,
      reload_time: 0.5,
    } as Flick;
    const kill = budget(AVERAGE, flick, 0)?.bars[0].segments ?? [];
    expect(kill.map((segment) => Math.round(1000 * segment.grow * 0.5))).toEqual([
      100, 200, 0, 0, 200,
    ]);
    expect(kill[4].title).toBe(
      'Reload 500 ms, 200 ms of it shown (taken from the confirmation and micro it overlapped)',
    );
  });

  it("has no reload step when the scenario's magazine never runs out", () => {
    const flick = { kill_number: 7, total: 0.5, parts: AVERAGE } as Flick;
    expect(budget(AVERAGE, flick)?.legend).toHaveLength(4);
    expect(averageReload({} as ClickSummary, [flick])).toBeNull();
    const reloads = { reloads: { count: 1, seconds: 0.5, score_lost: null } } as ClickSummary;
    const split = [
      { ...flick, reload_time: 0.5 },
      flick,
      { parts: undefined, reload_time: 0.8 } as Flick,
    ];
    expect(averageReload(reloads, split)).toBe(0.25);
  });

  it('is none without an average', () => {
    expect(budget(null, null)).toBeNull();
  });
});
