import {
  parseStatsCsv,
  parseVodName,
  stampSeconds,
  statsForVideo,
  StatsCsv,
  statsSummary,
} from './stats-csv';

const NAME = '1wall 2targets xsmall - Challenge - 2026.10.01-16.23.04 Stats.csv';
const TEXT = [
  'Kill #,Timestamp,Bot,Weapon,TTK,Shots,Hits',
  '1,16:22:20.833,target,pistol,0.000000s,1,1',
  '2,16:22:21.492,target,pistol,0.000000s,2,1',
  '',
  'Weapon,Shots,Hits',
  '16ML9 - 080,3,2',
  '',
  'Kills:,66',
  'Hit Count:,66',
  'Miss Count:,12',
  'Score:,558.461548',
  'Scenario:,1wall 2targets xsmall',
  'Challenge Start:,16:22:19.708',
].join('\r\n');

function stats(name: string): StatsCsv {
  return { name, meta: { Scenario: 'x' }, killRows: 0, text: '' };
}

describe('parseStatsCsv', () => {
  it('reads the key-value lines and counts the kill rows of the first table only', () => {
    const s = parseStatsCsv(NAME, TEXT);
    expect(s?.killRows).toBe(2);
    expect(s?.meta['Score']).toBe('558.461548');
    expect(s?.meta['Challenge Start']).toBe('16:22:19.708');
  });

  it('turns down a file that is not a stats file', () => {
    expect(parseStatsCsv('notes.csv', 'a,b\n1,2\n')).toBeNull();
  });
});

describe('statsSummary', () => {
  it('gives the score, kills, accuracy and the time from the name', () => {
    const s = statsSummary(parseStatsCsv(NAME, TEXT) as StatsCsv);
    expect(s).toEqual({
      scenario: '1wall 2targets xsmall',
      score: 558.461548,
      kills: 66,
      accuracy: 66 / 78,
      stamp: '2026.10.01-16.23.04',
    });
  });

  it('counts the kill rows when the file has no Kills line, and has no accuracy without shots', () => {
    const s = statsSummary({ name: 'x.csv', meta: { Scenario: 'x' }, killRows: 3, text: '' });
    expect(s.kills).toBe(3);
    expect(s.accuracy).toBeNull();
    expect(s.stamp).toBeNull();
  });
});

describe('parseVodName and stampSeconds', () => {
  it("reads KovOBS's names", () => {
    expect(parseVodName('Air - 1931.23 - 2026.10.01-16.23.03.mkv')).toEqual({
      scenario: 'Air',
      score: 1931.23,
      stamp: '2026.10.01-16.23.03',
    });
    expect(parseVodName('Replay 2026-10-02 14-28-02.mp4')).toBeNull();
  });

  it('reads the year 0026 as 2026', () => {
    expect(stampSeconds('0026.06.01-10.00.00')).toBe(stampSeconds('2026.06.01-10.00.00'));
    expect(stampSeconds('not a time')).toBeNull();
  });
});

describe('statsForVideo', () => {
  const video = '1wall 2targets xsmall - 558 - 2026.10.01-16.23.01.mp4';

  it('picks the stats file of the same scenario within five seconds', () => {
    const near = stats(NAME);
    const other = stats('1wall 2targets xsmall - Challenge - 2026.10.01-16.21.38 Stats.csv');
    expect(statsForVideo(video, [other, near], false)).toBe(near);
  });

  it('takes the only stats file for the only video, whatever its name', () => {
    const only = stats('mine.csv');
    expect(statsForVideo('clip.mkv', [only], true)).toBe(only);
    expect(statsForVideo('clip.mkv', [only], false)).toBeNull();
  });

  it('pairs nothing when no name matches among several files', () => {
    expect(statsForVideo(video, [stats('a.csv'), stats('b.csv')], true)).toBeNull();
  });
});
