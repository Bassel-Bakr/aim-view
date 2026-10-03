import { isClickReport, Report } from './api';

describe('isClickReport', () => {
  it('takes a hold run (a lightning gun) for a clicking run, as the old page did', () => {
    const report = (mode: string) => ({ mode }) as unknown as Report;
    expect(isClickReport(report('click'))).toBe(true);
    expect(isClickReport(report('hold'))).toBe(true);
    expect(isClickReport(report('track'))).toBe(false);
    expect(isClickReport(null)).toBe(false);
    expect(isClickReport(undefined)).toBe(false);
  });
});
