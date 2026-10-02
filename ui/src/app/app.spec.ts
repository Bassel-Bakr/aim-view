import { TestBed } from '@angular/core/testing';
import { App } from './app';

async function render(fetchImpl: typeof fetch) {
  vi.stubGlobal('fetch', fetchImpl);
  await TestBed.configureTestingModule({ imports: [App] }).compileComponents();
  const fixture = TestBed.createComponent(App);
  await fixture.whenStable();
  return fixture.nativeElement as HTMLElement;
}

describe('App', () => {
  afterEach(() => vi.unstubAllGlobals());

  it('shows the model the review uses and where it runs', async () => {
    const el = await render(async () => new Response(JSON.stringify({ chosen: 'full_v3', device: 'cuda', models: [] })));
    expect(el.querySelector('.pill')?.textContent).toBe('full_v3 · GPU');
  });

  it('says so when the review server is not running', async () => {
    const el = await render(async () => {
      throw new TypeError('Failed to fetch');
    });
    expect(el.querySelector('[role=status]')?.textContent).toContain('not running');
  });
});
