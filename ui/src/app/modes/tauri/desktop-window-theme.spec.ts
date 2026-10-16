import { HttpRequest, provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { answer } from '../../fake-api';
import { Theme } from '../../services/theme';
import { DesktopWindowTheme } from './desktop-window-theme';

describe('DesktopWindowTheme', () => {
  beforeEach(() =>
    TestBed.configureTestingModule({
      providers: [provideHttpClient(), provideHttpClientTesting()],
    }),
  );
  afterEach(() => localStorage.clear());

  it('tells the window the theme choice at start and after each change', async () => {
    const sent: (string | null)[] = [];
    const window = (req: HttpRequest<unknown>) => {
      if (req.method === 'POST') sent.push(req.params.get('theme'));
      return { theme: req.params.get('theme') };
    };
    TestBed.inject(DesktopWindowTheme);
    TestBed.tick();
    await answer({ '/api/window/theme': window });
    TestBed.inject(Theme).choose('dark');
    TestBed.tick();
    await answer({ '/api/window/theme': window });
    expect(sent).toEqual(['system', 'dark']);
  });
});
