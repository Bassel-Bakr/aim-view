import { resource, ResourceRef, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { recording } from '../../fake-api';
import { mouseRun } from '../../fake-mouse';
import { MouseLoggerState, MouseMeasures } from '../../mouse-api';
import { MouseLogs } from '../../platform/mouse-logs';
import { MousePanel, mouseCards, mouseKillRows, mouseNotes, mouseSource } from './mouse-panel';

/** Mouse logs standing in for a mode: the browser's way (logs added) or the desktop app's (logs made). */
class StandInLogs extends MouseLogs {
  readonly measured = signal<MouseMeasures | null>(null);
  added: File[] = [];

  constructor(
    readonly adds: boolean,
    readonly logs: boolean,
  ) {
    super();
  }

  measures(): ResourceRef<MouseMeasures | null | undefined> {
    return resource({
      params: () => this.measured(),
      loader: ({ params }) => Promise.resolve(params),
    });
  }

  add(_id: string, file: File): Promise<MouseMeasures> {
    this.added.push(file);
    const m: MouseMeasures = { file: file.name, run: mouseRun(), error: null };
    this.measured.set(m);
    return Promise.resolve(m);
  }

  forget(): Promise<void> {
    this.measured.set(null);
    return Promise.resolve();
  }

  logger(): ResourceRef<MouseLoggerState | null | undefined> {
    return resource({ loader: () => Promise.resolve(null) });
  }

  setLogger(): Promise<MouseLoggerState> {
    return Promise.reject(new Error('no logger'));
  }
}

async function render(logs: StandInLogs): Promise<HTMLElement> {
  TestBed.configureTestingModule({ providers: [{ provide: MouseLogs, useValue: logs }] });
  const fixture = TestBed.createComponent(MousePanel);
  fixture.componentRef.setInput('recording', recording({}));
  await fixture.whenStable();
  return fixture.nativeElement as HTMLElement;
}

describe('MousePanel', () => {
  it("shows the run's medians with their spread, and each kill", async () => {
    const logs = new StandInLogs(true, false);
    logs.measured.set({ file: 'mouse_a.bin', run: mouseRun(), error: null });
    const el = await render(logs);
    const cards = [...el.querySelectorAll('.card')].map((c) =>
      [...c.children].map((s) => s.textContent?.trim()).join(' | '),
    );
    expect(cards).toEqual([
      'Reaction | 163 ms | p10 150 ms · p90 170 ms',
      'Peak speed | 383 °/s | p10 300 °/s · p90 400 °/s',
      'Still before the click | 41 ms | p10 0 ms · p90 80 ms',
    ]);
    const rows = [...el.querySelectorAll('tbody tr')].map((r) =>
      [...r.children].map((c) => c.textContent?.trim()).join(' | '),
    );
    expect(rows[1]).toBe(
      '2 | 04:54:22.001 | 163 ms | – | 383 °/s | – | 0 ms | 136 °/s | 21.6° | –',
    );
    expect(el.textContent).toContain('mouse_a.bin: 2 of 2 kills matched with a click');
  });

  it('offers to add a log where the browser reads them, and shows it once added', async () => {
    const logs = new StandInLogs(true, false);
    const el = await render(logs);
    expect(el.textContent).toContain('No mouse log for this run');
    const input = el.querySelector('input[type=file]') as HTMLInputElement;
    const file = new File(['x'], 'mouse_b.bin');
    Object.defineProperty(input, 'files', { value: [file] });
    input.dispatchEvent(new Event('change'));
    for (let i = 0; i < 3; i++) {
      await new Promise((r) => setTimeout(r));
      TestBed.tick();
    }
    expect(logs.added).toEqual([file]);
    expect(el.textContent).toContain('mouse_b.bin: 2 of 2 kills');
  });

  it('says why a log measured nothing, and shows nothing where no log can be had', async () => {
    const logs = new StandInLogs(false, true);
    logs.measured.set({
      file: 'mouse_c.bin',
      run: null,
      error: 'no left-button press lies within 1 s of any kill',
    });
    expect((await render(logs)).textContent).toContain('mouse_c.bin: no left-button press');
    TestBed.resetTestingModule();
    expect((await render(new StandInLogs(false, false))).textContent?.trim()).toBe('');
  });

  it('words the numbers as the reader prints them', () => {
    const run = mouseRun({ log: { ...mouseRun().log, throttled: true, median_interval: 0.008 } });
    expect(mouseCards(run)[0].why).toContain('(median of 2 kills)');
    expect(mouseKillRows(run)[0].cells[0]).toBe('163 ms');
    expect(mouseSource(null, run)).toBe(
      'The log: 2 of 2 kills matched with a click (p90 0.7 ms apart), 1 miss. Logged 04:54:20 to 04:54:28, 125 events a second at the median.',
    );
    expect(mouseNotes(run)).toContain(
      '1,600 dpi and 70 cm/360 (from the stats file); speeds over 4 ms',
    );
  });
});
