import { TestBed } from '@angular/core/testing';
import { ClickReport as ClickReportData, Flick } from '../../api';
import { answer, serverMode } from '../../fake-api';
import { FlickFocus } from '../flick-focus';
import { ClickReport } from './click-report';

const FLICK = { n: 4, D0: 8, dir: 0, total: 0.4, end_left: 0, parts: null, shots: 1 } as Flick;
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

describe('ClickReport', () => {
  it("shows the whole run's cards, then a picked kill's, and goes back", async () => {
    TestBed.configureTestingModule({
      providers: serverMode(),
    });
    const fixture = TestBed.createComponent(ClickReport);
    fixture.componentRef.setInput('report', REPORT);
    await answer({ '/api/vods': [] });
    await fixture.whenStable();
    const el = fixture.nativeElement as HTMLElement;
    const heading = () => el.querySelector('section')?.textContent;
    expect(heading()).toContain('Whole run');
    expect(el.querySelectorAll('h3')[1].textContent).toBe('What to look at');
    expect(el.textContent?.indexOf('Stopped short')).toBeLessThan(
      el.textContent?.indexOf('Slow start') ?? 0,
    );

    TestBed.inject(FlickFocus).selected.set(FLICK);
    await fixture.whenStable();
    expect(heading()).toContain('Kill 4');
    expect(el.querySelector('h3')?.textContent).toBe("Where kill 4's 400 ms goes");

    (el.querySelector('section button') as HTMLButtonElement).click();
    await fixture.whenStable();
    expect(heading()).toContain('Whole run');
  });
});
