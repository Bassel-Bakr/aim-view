import { HttpRequest } from '@angular/common/http';
import { TestBed } from '@angular/core/testing';
import { StatsChoice, StatsPairing } from '../../api';
import { answer, ApiRoutes, recording, serverMode } from '../../fake-api';
import { Library } from '../../services/library';
import { StatsFile } from './stats-file';

const ID = 'Air/Air - 1 - 2026.10.01-16.23.03.mp4';
const PAIRING: StatsPairing = {
  file: null,
  how: 'missing',
  scenario: 'Air',
  candidates: [
    {
      name: 'Air - Challenge - 2026.10.01-16.23.04 Stats.csv',
      scenario: 'Air',
      stamp: '2026.10.01-16.23.04',
      off: 1,
    },
    {
      name: 'Air - Challenge - 2026.10.01-16.21.38 Stats.csv',
      scenario: 'Air',
      stamp: '2026.10.01-16.21.38',
      off: -85,
    },
  ],
};

async function render(routes: ApiRoutes): Promise<HTMLElement> {
  TestBed.configureTestingModule({ providers: serverMode() });
  TestBed.inject(Library).selectedId.set(ID);
  const fixture = TestBed.createComponent(StatsFile);
  fixture.componentRef.setInput('recording', recording({ id: ID, scenario: 'Air', stats: false }));
  await answer(routes);
  await fixture.whenStable();
  return fixture.nativeElement as HTMLElement;
}

describe('StatsFile', () => {
  afterEach(() => history.replaceState(null, '', '/'));

  it("lists the scenario's stats files, nearest the recording's time first, with how far each is", async () => {
    const el = await render({
      '/api/vods': [],
      '/api/stats': PAIRING,
      '/api/job': { stage: 'none' },
    });
    expect(el.textContent).toContain('No stats file · none found by its name and time');
    const rows = [...el.querySelectorAll('li')].map((li) =>
      [...li.children].map((cell) => cell.textContent?.trim()).join(' | '),
    );
    expect(rows).toEqual(['Oct 1, 16:23 | 1 s after | Use', 'Oct 1, 16:21 | 1 min before | Use']);
  });

  it('pairs the recording with the file picked, and the list says it has a stats file now', async () => {
    let sent: StatsChoice | null = null;
    const routes: ApiRoutes = {
      '/api/vods': [recording({ id: ID, scenario: 'Air', stats: false })],
      '/api/job': { stage: 'none' },
      '/api/stats': (req: HttpRequest<unknown>) => {
        if (req.method === 'GET')
          return sent ? { ...PAIRING, file: PAIRING.candidates[0].name, how: 'picked' } : PAIRING;
        sent = req.body as StatsChoice;
        return { job: { stage: 'none' }, stats: true };
      },
    };
    const el = await render(routes);
    el.querySelector<HTMLButtonElement>('li button')?.click();
    await answer(routes);
    expect(sent).toEqual({ file: PAIRING.candidates[0].name, source: 'kovaak' });
    expect(TestBed.inject(Library).selected()?.stats).toBe(true);
    await answer(routes);
    expect(el.textContent).toContain('your pick');
    expect(el.querySelector('li')?.textContent).toContain('in use');
  });
});
