import { formatOffset, formatStamp } from './format';

describe('formatStamp', () => {
  it('leaves out this year', () => {
    expect(formatStamp('2026.08.11-13.55.27', 2026)).toBe('Aug 11, 13:55');
  });

  it('shows another year', () => {
    expect(formatStamp('2025.12.01-09.05.00', 2026)).toBe('Dec 1 2025, 09:05');
  });

  it('reads the year 0026 that some KovOBS recordings carry as 2026', () => {
    expect(formatStamp('0026.06.30-01.48.48', 2026)).toBe('Jun 30, 01:48');
  });

  it('leaves text that is not a time stamp as it is', () => {
    expect(formatStamp('upload', 2026)).toBe('upload');
  });
});

describe('formatOffset', () => {
  it('says how far, in the unit that fits, and which way', () => {
    expect(formatOffset(0.4)).toBe('at the same time');
    expect(formatOffset(12)).toBe('12 s after');
    expect(formatOffset(-85)).toBe('1 min before');
    expect(formatOffset(7200)).toBe('2 h after');
    expect(formatOffset(-86400)).toBe('1 day before');
    expect(formatOffset(-27e6)).toBe('313 days before');
  });
});
