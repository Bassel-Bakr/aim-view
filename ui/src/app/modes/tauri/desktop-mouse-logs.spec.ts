import { HttpRequest, provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { answer } from '../../fake-api';
import { mouseRun } from '../../fake-mouse';
import { MouseLoggerState, MouseMeasures } from '../../mouse-api';
import { DesktopMouseLogs } from './desktop-mouse-logs';

const STATE: MouseLoggerState = {
  available: true,
  on: false,
  file: null,
  since: null,
  folder: 'C:/Users/me/AppData/Roaming/aimview/mouse',
  throttle: 'background throttle not set',
  last: null,
  error: null,
};

describe('DesktopMouseLogs', () => {
  beforeEach(() =>
    TestBed.configureTestingModule({
      providers: [provideHttpClient(), provideHttpClientTesting()],
    }),
  );

  it("asks the app for the recording's measures", async () => {
    const logs = TestBed.inject(DesktopMouseLogs);
    const measured: MouseMeasures = {
      file: 'mouse_2026-10-03_16-20-11.bin',
      run: mouseRun(),
      error: null,
    };
    let asked = '';
    const measures = TestBed.runInInjectionContext(() => logs.measures(() => 'a/b.mp4'));
    await answer({
      '/api/mouse': (req: HttpRequest<unknown>) => {
        asked = req.params.get('id') ?? '';
        return measured;
      },
    });
    expect(asked).toBe('a/b.mp4');
    expect(measures.value()?.run?.matched).toBe(2);
  });

  it('turns the logger on and says how it stands', async () => {
    const logs = TestBed.inject(DesktopMouseLogs);
    const state = TestBed.runInInjectionContext(() => logs.logger());
    let on: string | null = null;
    const turned = logs.setLogger(true);
    await answer({
      '/api/mouse/logger': (req: HttpRequest<unknown>) => {
        if (req.method === 'POST') on = req.params.get('on');
        return req.method === 'POST'
          ? { ...STATE, on: true, since: 1, file: 'mouse_x.bin' }
          : STATE;
      },
    });
    expect(on).toBe('1');
    expect((await turned).on).toBe(true);
    expect(state.value()?.on).toBe(false);
  });
});
