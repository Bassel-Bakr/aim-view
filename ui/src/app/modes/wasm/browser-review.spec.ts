import { TestBed } from '@angular/core/testing';
import { MODE } from '../mode.browser';
import { LocalFiles } from '../web-files/local-files';
import { StatsFolder } from '../web-files/stats-folder';
import { BrowserReview } from './browser-review';

const STATS = 'Kill #,Timestamp\n1,16:22:20.833\n\nKills:,1\nScore:,30.42\nScenario:,Probe\n';

function file(name: string, text = 'x'): File {
  return new File([text], name, { lastModified: Date.UTC(2026, 9, 2, 12, 0, 0) });
}

const NO_STATS = 'No stats file';

function setUp(): BrowserReview {
  TestBed.configureTestingModule({ providers: MODE.providers });
  return TestBed.inject(BrowserReview);
}

describe('BrowserReview', () => {
  it('reviews a run without a stats file, and says what it reads the kills from', async () => {
    const review = setUp();
    const [id] = (await TestBed.inject(LocalFiles).add([file('Probe - 2026.09.27-19.31.47.mp4')]))
      .ids;
    expect(review.unavailable(id)).toBeNull();
    expect(review.caveat(id)).toContain(NO_STATS);
    expect(review.caveat(id)).toContain('reads the kills from the HUD, or from the video alone');
    expect(review.caveat(id)).toContain('exact review');
  });

  it("says nothing of the stats file once KovaaK's stats folder is given, or the run has one", async () => {
    const review = setUp();
    const local = TestBed.inject(LocalFiles);
    const [paired] = (
      await local.add([
        file('Probe - 30.42 - 2026.09.27-19.31.47.mp4'),
        file('Probe - Challenge - 2026.09.27-19.31.50 Stats.csv', STATS),
      ])
    ).ids;
    expect(review.unavailable(paired)).toBeNull();
    expect(review.caveat(paired) ?? '').not.toContain(NO_STATS);
    const [alone] = (await local.add([file('Probe - 2026.09.28-10.00.00.mp4')])).ids;
    expect(review.caveat(alone)).toContain(NO_STATS);
    TestBed.inject(StatsFolder).index([]);
    expect(review.caveat(alone) ?? '').not.toContain(NO_STATS);
  });

  it('cannot review a recording that is not open here', () => {
    expect(setUp().unavailable('not/open.mp4')).toBe('The recording is not open in this browser.');
  });
});
