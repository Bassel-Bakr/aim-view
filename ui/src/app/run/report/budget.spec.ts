import { Flick, KillParts } from '../../api';
import { budget } from './budget';

const AVERAGE: KillParts = [0.1, 0.2, 0.1, 0.05, 0.05];

describe('budget', () => {
  it("shows the average kill's steps, named inside the bar where they fit", () => {
    const b = budget(AVERAGE, null);
    expect(b?.title).toBe("Where an average kill's 500 ms goes");
    expect(b?.bars[0].segments.map((s) => s.text)).toEqual([
      'React 100 ms',
      'Main flick 200 ms',
      'Onto the target 100 ms',
      '',
      '',
    ]);
  });

  it('puts a picked kill over the average on one time scale', () => {
    const flick = { n: 7, total: 1, parts: [0.2, 0.4, 0.2, 0.1, 0.1] } as Flick;
    const b = budget(AVERAGE, flick);
    expect(b?.title).toBe("Where kill 7's 1000 ms goes");
    expect(b?.bars.map((x) => x.width)).toEqual([100, 50]);
    expect(b?.bars[1].label).toBe('Average kill, 500 ms');
    expect(b?.legend[0]).toMatchObject({ text: 'React 200 ms', average: '(avg 100 ms)' });
  });

  it("says so when a kill's steps could not be split", () => {
    const b = budget(AVERAGE, { n: 3, total: 0.8, parts: null } as Flick);
    expect(b?.note).toContain('was not found');
    expect(b?.bars).toHaveLength(1);
  });

  it('is none without an average', () => {
    expect(budget(null, null)).toBeNull();
  });
});
