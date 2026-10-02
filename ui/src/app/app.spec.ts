import { TestBed } from '@angular/core/testing';
import { App } from './app';
import { answer, ApiRoutes, NO_SERVER, recording, serverMode } from './fake-api';

async function render(routes: ApiRoutes): Promise<HTMLElement> {
  TestBed.configureTestingModule({
    imports: [App],
    providers: serverMode(),
  });
  const fixture = TestBed.createComponent(App);
  await answer(routes);
  await fixture.whenStable();
  return fixture.nativeElement as HTMLElement;
}

describe('App', () => {
  afterEach(() => history.replaceState(null, '', '/'));

  it('shows the model the review uses and where it runs', async () => {
    const el = await render({
      '/api/models': {
        chosen: 'full_v3',
        device: 'cuda',
        speed: '',
        checked_on: '',
        checks: [],
        models: [],
      },
      '/api/vods': [],
    });
    expect(el.querySelector('app-model-panel button')?.textContent?.trim()).toBe('full_v3 · GPU');
  });

  it('says so when the review server is not running', async () => {
    const el = await render({ '/api/models': NO_SERVER, '/api/vods': NO_SERVER });
    expect(el.querySelector('.top [role=status]')?.textContent).toContain('not running');
  });

  it('opens the recording the link names', async () => {
    const r = recording({ id: 'x/Controlsphere.mp4', scenario: 'Controlsphere', kind: 'tracking' });
    history.replaceState(null, '', `/?id=${encodeURIComponent(r.id)}`);
    const el = await render({
      '/api/vods': [r],
      '/api/report': null,
      '/api/job': { stage: 'none' },
    });
    expect(el.querySelector('app-run h2')?.textContent).toContain('Controlsphere');
    expect(el.querySelector('app-run h2')?.textContent).toContain('Tracking');
  });
});
