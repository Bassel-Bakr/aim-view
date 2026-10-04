import { TestBed } from '@angular/core/testing';
import { ClickReport as ClickReportData, ClickWhatIf, Flick } from '../../api';
import { answer, serverMode } from '../../fake-api';
import { FlickFocus } from '../flick-focus';
import { ClickReport } from './click-report';

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
    const titles = () => [...el.querySelectorAll('h3')].map((h) => h.textContent);
    expect(heading()).toContain('Whole run');
    expect(titles()).toContain('The run at a glance');

    TestBed.inject(FlickFocus).selected.set(FLICK);
    await fixture.whenStable();
    expect(heading()).toContain('Kill 4');

    (el.querySelector('section button') as HTMLButtonElement).click();
    await fixture.whenStable();
    expect(heading()).toContain('Whole run');
  });

  it('shows what would raise the score only when the report has the lines', async () => {
    TestBed.configureTestingModule({ providers: serverMode() });
    const fixture = TestBed.createComponent(ClickReport);
    fixture.componentRef.setInput('report', REPORT);
    await answer({ '/api/vods': [] });
    await fixture.whenStable();
    const el = fixture.nativeElement as HTMLElement;
    const section = () => el.querySelector('section[aria-label="What would raise your score"]');
    expect(section()).toBeNull();

    const line: ClickWhatIf = {
      group: 'pace',
      what: 'Start sooner',
      kills: 3,
      score: null,
      how: 'Why.',
    };
    fixture.componentRef.setInput('report', {
      ...REPORT,
      summary: { ...REPORT.summary, what_if: [line] },
    });
    await fixture.whenStable();
    expect(section()?.textContent).toContain('Pace');
    expect(section()?.textContent).toContain('+3.0 kills');
  });
});
