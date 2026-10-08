import { TestBed } from '@angular/core/testing';
import { ClickReport as ClickReportData, Flick } from '../../api';
import { answer, serverMode } from '../../fake-api';
import { FlickFocus } from '../flick-focus';
import { ClickSide } from './click-side';

const FLICK = {
  kill_number: 4,
  D0: 8,
  direction_deg: 0,
  total: 0.4,
  end_left: 0,
  parts: undefined,
  shots: 1,
} as Flick;
const REPORT = {
  mode: 'click',
  fps: 120,
  flicks: [FLICK],
  paths: {},
  issues: [
    { issue: 1, title: 'Slow start', value: '75 ms', flag: 'fine', why: 'w' },
    { issue: 2, title: 'Stopped short', value: '40%', flag: 'attention', why: 'w' },
  ],
  summary: {
    kills: 1,
    score: 10,
    misses: 0,
    radius: 0.5,
    measured: 1,
    budget: [0.1, 0.1, 0.1, 0.1, 0.1],
    by_distance: [],
    by_direction: [],
    info: { source: 'stats', matched: 1, kills_stats: 1 },
  },
} as unknown as ClickReportData;

describe('ClickSide', () => {
  it("lists the checks to work on first, and shows where the picked kill's time went", async () => {
    TestBed.configureTestingModule({
      providers: serverMode(),
    });
    const fixture = TestBed.createComponent(ClickSide);
    fixture.componentRef.setInput('report', REPORT);
    await answer({ '/api/vods': [] });
    await fixture.whenStable();
    const el = fixture.nativeElement as HTMLElement;
    const titles = () =>
      [...el.querySelectorAll('h3')].map((titleElement) => titleElement.textContent?.trim());
    expect(titles()).toContain('Checks');
    expect(el.textContent).toContain('1 to work on · 1 fine');
    expect(el.textContent?.indexOf('Stopped short')).toBeLessThan(
      el.textContent?.indexOf('Slow start') ?? 0,
    );
    // the average's reference bar keeps its space, hidden, until a kill is picked
    expect(el.querySelector('.reference[data-hidden]')?.textContent).toContain('Average kill');

    TestBed.inject(FlickFocus).selected.set(FLICK);
    await fixture.whenStable();
    expect(titles()).toContain("Where kill 4's 400 ms goes");
  });
});
