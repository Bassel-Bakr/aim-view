import { Flick, FlickProfile } from '../../api';
import { killCurve, PROFILE_BOX, profileChart } from './flick-profile-model';

/** A profile of 0 to 125% in 6 points, 25% apart, rising to its top at 50%. */
const profile: FlickProfile = {
  n: 12,
  step: 0.25,
  mean: [0.4, 0.8, 1, 0.6, 0.1, 0],
  p25: [0.3, 0.7, 0.9, 0.5, 0.05, 0],
  p75: [0.5, 0.9, 1, 0.7, 0.2, 0.05],
  peak_at: 0.5,
  braking: 0.375,
};

const plotRight = PROFILE_BOX.width - PROFILE_BOX.right;
const plotBottom = PROFILE_BOX.height - PROFILE_BOX.bottom;

describe('flick profile chart', () => {
  it('has no chart without a profile', () => {
    expect(profileChart(undefined)).toBeNull();
    expect(profileChart(null)).toBeNull();
  });

  it('spans the profile, marks the flick end and the top speed, and says both numbers', () => {
    const c = profileChart(profile)!;
    expect(c.span).toBe(1.25);
    expect(c.xTicks.map((t) => t.label)).toEqual(['0%', '25%', '50%', '75%', '100%', '125%']);
    expect(c.xTicks[0].at).toBe(PROFILE_BOX.left);
    expect(c.xTicks[5].at).toBeCloseTo(plotRight, 6);
    expect(c.end).toBeCloseTo(c.xTicks[4].at, 6);
    // the top speed sits on the average at 50%, its highest point
    expect(c.peak.x).toBeCloseTo(c.xTicks[2].at, 6);
    expect(c.peak.y).toBeCloseTo(c.yTicks[4].at, 6);
    expect(c.yTicks[0].at).toBe(plotBottom);
    expect(c.summary).toBe('Flick speed tops out at 50% of the flick; braking takes 38%.');
    // the band goes out along the 75th percentile and back along the 25th, closed
    expect(c.band.match(/[ML]/g)).toHaveLength(12);
    expect(c.band.endsWith('Z')).toBe(true);
    expect(c.mean.startsWith(`M${PROFILE_BOX.left.toFixed(1)},`)).toBe(true);
  });

  it("draws a kill's own curve as shares of its top speed, cut at the plot's edge", () => {
    const c = profileChart(profile)!;
    const kill = { n: 3, speed: { v: [100, 300, 400, 200, 50, 20, 10], end: 4 } } as Flick;
    const d = killCurve(kill, c)!;
    const points = d
      .slice(1)
      .split('L')
      .map((p) => p.split(',').map(Number));
    // 0 to 125% of a 4-frame flick: 6 frames, the seventh (150%) left out
    expect(points).toHaveLength(6);
    expect(points[0][0]).toBeCloseTo(PROFILE_BOX.left, 1);
    expect(points[4][0]).toBeCloseTo(c.end, 1);
    // its top (400 °/s, at 50%) on the 100% line
    expect(points[2][1]).toBeCloseTo(c.yTicks[4].at, 1);
    expect(killCurve(null, c)).toBeNull();
    expect(killCurve({ n: 4 } as Flick, c)).toBeNull();
  });
});
