import { HttpRequest, provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { answer } from '../fake-api';
import { DesktopMouseLogs } from '../modes/tauri/desktop-mouse-logs';
import { MouseLoggerState } from '../mouse-api';
import { MouseLogs } from '../platform/mouse-logs';
import { loggerStatus, MouseSwitch } from './mouse-switch';

const OFF: MouseLoggerState = {
  available: true,
  on: false,
  file: null,
  since: null,
  folder: 'C:/data/mouse',
  throttle: 'background throttle not set',
  last: null,
  error: null,
};

describe('MouseSwitch', () => {
  it("turns the desktop app's logger on", async () => {
    TestBed.configureTestingModule({
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        { provide: MouseLogs, useExisting: DesktopMouseLogs },
      ],
    });
    const fixture = TestBed.createComponent(MouseSwitch);
    let state = OFF;
    const routes = {
      '/api/mouse/logger': (req: HttpRequest<unknown>) => {
        if (req.method === 'POST') state = { ...OFF, on: req.params.get('on') === '1', since: 0 };
        return state;
      },
    };
    await answer(routes);
    const el = fixture.nativeElement as HTMLElement;
    const button = el.querySelector('[role=switch]') as HTMLButtonElement;
    expect(button.getAttribute('aria-checked')).toBe('false');
    expect(button.dataset['tooltip']).toContain('into C:/data/mouse. background throttle not set');
    button.click();
    await answer(routes);
    await fixture.whenStable();
    expect(button.getAttribute('aria-checked')).toBe('true');
    expect(el.textContent).toContain('logging since');
  });

  it('says what the last log holds, and why the logger failed', () => {
    expect(loggerStatus(OFF)).toBeNull();
    expect(
      loggerStatus({
        ...OFF,
        last: { file: 'a.bin', events: 1234, duration: 61.4, throttled: true },
      }),
    ).toEqual({ text: 'last log: 1,234 events in 61 s, throttled by Windows', failed: false });
    expect(loggerStatus({ ...OFF, error: 'the logger stopped' })).toEqual({
      text: 'the logger stopped',
      failed: true,
    });
  });
});
