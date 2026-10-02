import { TestBed } from '@angular/core/testing';
import { App } from './app';
import { fakeFetch, recording } from './testing';

async function render(fetchImpl: typeof fetch): Promise<HTMLElement> {
  vi.stubGlobal('fetch', fetchImpl);
  await TestBed.configureTestingModule({ imports: [App] }).compileComponents();
  const fixture = TestBed.createComponent(App);
  await fixture.whenStable();
  return fixture.nativeElement as HTMLElement;
}

describe('App', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    history.replaceState(null, '', '/');
  });

  it('shows the model the review uses and where it runs', async () => {
    const el = await render(
      fakeFetch({
        '/api/models': { chosen: 'full_v3', device: 'cuda', models: [] },
        '/api/vods': [],
      }),
    );
    expect(el.querySelector('.top .pill')?.textContent).toBe('full_v3 · GPU');
  });

  it('says so when the review server is not running', async () => {
    const el = await render(async () => {
      throw new TypeError('Failed to fetch');
    });
    expect(el.querySelector('.top [role=status]')?.textContent).toContain('not running');
  });

  it('opens the recording the link names', async () => {
    const r = recording({ id: 'x/Controlsphere.mp4', scenario: 'Controlsphere', kind: 'tracking' });
    history.replaceState(null, '', `/?id=${encodeURIComponent(r.id)}`);
    const el = await render(fakeFetch({ '/api/vods': [r] }));
    expect(el.querySelector('app-run-header h2')?.textContent).toContain('Controlsphere');
    expect(el.querySelector('app-run-header h2')?.textContent).toContain('Tracking');
  });
});
