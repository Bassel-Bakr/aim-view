import { Component, computed, inject, input, signal } from '@angular/core';
import { Button } from '../../controls/button';
import { errorMessage, Recording } from '../../api';
import { formatDegrees, formatNumber, formatSpeed } from '../../format';
import { MouseKill, MouseMeasureKey, MouseRun } from '../../mouse-api';
import { MouseLogs } from '../../platform/mouse-logs';

/** How the panel names a measure, what it means, and how its values are written. */
export interface MeasureLabel {
  label: string;
  why: string;
  write: (v: number) => string;
}

/** A measure's card: its median, its label, its p10 and p90, and what it means. */
export interface MouseCard {
  key: MouseMeasureKey;
  value: string;
  label: string;
  detail: string;
  why: string;
}

/** One kill's row in the table of kills. */
export interface MouseKillRow {
  n: number;
  at: string;
  cells: string[];
}

/** What the last action did; failed when it could not be done. */
export interface MouseMessage {
  text: string;
  failed: boolean;
}

const ms = (v: number) => `${Math.round(v)} ms`;
const speed = (v: number) => formatSpeed(v);
const degrees = (v: number) => formatDegrees(v, 1);
const NONE = '–';

export const MEASURES: Record<MouseMeasureKey, MeasureLabel> = {
  reaction_ms: {
    label: 'Reaction',
    why: 'From the click before until the mouse starts moving: its speed first reaches the start speed',
    write: ms,
  },
  flick_ms: {
    label: 'Flick',
    why: 'From the start until the mouse stops: its speed stays under the stop speed for the hold time',
    write: ms,
  },
  peak_dps: { label: 'Peak speed', why: 'The highest speed in the flick', write: speed },
  stop_to_click_ms: {
    label: 'Stop to click',
    why: 'From the stop to the click, with any corrections after the flick',
    write: ms,
  },
  still_ms: {
    label: 'Still before the click',
    why: 'How long the crosshair sat still on the target before the click; 0 when the click came while it moved',
    write: ms,
  },
  click_dps: {
    label: 'Speed at the click',
    why: 'The speed over the moments just before the click',
    write: speed,
  },
  dist_deg: {
    label: 'Distance',
    why: 'How far the crosshair moved from the click before',
    write: degrees,
  },
};

/** The table's columns after the kill and its time, with the field each one shows. */
const KILL_COLUMNS: readonly MouseMeasureKey[] = [
  'reaction_ms',
  'flick_ms',
  'peak_dps',
  'stop_to_click_ms',
  'still_ms',
  'click_dps',
  'dist_deg',
];

/** The run's measures as cards: the median of each, with its p10 and p90. */
export function mouseCards(run: MouseRun): MouseCard[] {
  return run.spreads.map((s) => {
    const m = MEASURES[s.key];
    return {
      key: s.key,
      value: m.write(s.median),
      label: m.label,
      detail: `p10 ${m.write(s.p10)} · p90 ${m.write(s.p90)}`,
      why: `${m.why} (median of ${s.n} kills)`,
    };
  });
}

/** Each kill's row: its number, the click's local time, and its measures. */
export function mouseKillRows(run: MouseRun): MouseKillRow[] {
  return run.kills.map((k: MouseKill) => ({
    n: k.n,
    at: k.press_local.slice(0, 12),
    cells: [
      ...KILL_COLUMNS.map((key) => {
        const v = k[key];
        return v == null ? NONE : MEASURES[key].write(v);
      }),
      k.corrections == null ? NONE : String(k.corrections),
    ],
  }));
}

/** Where the numbers come from: the log, the kills matched with clicks, the misses, and the event rate. */
export function mouseSource(file: string | null, run: MouseRun): string {
  const misses = run.misses_s.length;
  const rate = run.log.median_interval
    ? `, ${formatNumber(Math.round(1 / run.log.median_interval))} events a second at the median`
    : '';
  return (
    `${file ?? 'The log'}: ${run.matched} of ${run.kill_count} kills matched with a click ` +
    `(p90 ${run.gap_p90_ms.toFixed(1)} ms apart), ${misses} ${misses === 1 ? 'miss' : 'misses'}. ` +
    `Logged ${run.log.start_local.slice(0, 8)} to ${run.log.end_local.slice(0, 8)}${rate}.`
  );
}

/** The kinds of kill the reader counts, and the settings it measured with. */
export function mouseNotes(run: MouseRun): string {
  const n = run.kills.length;
  return (
    `Clicked while moving: ${run.moving_clicks} of ${n}; no stop before the click: ${run.no_stop}; ` +
    `with corrections: ${run.corrected}. ${formatNumber(run.dpi)} dpi and ${formatNumber(run.cm360)} cm/360 ` +
    `(from ${run.sens_from}); speeds over ${formatNumber(run.window_ms)} ms; moving from ` +
    `${formatNumber(run.start_dps)} °/s, still under ${formatNumber(run.stop_dps)} °/s for ` +
    `${formatNumber(run.hold_ms)} ms.`
  );
}

/**
 * The open recording's run measured from the raw mouse log (src/mouse.rs, python/mouse_read.py): each flick's start,
 * stop and peak speed, and how long the crosshair sat still before each click. Where the browser reads the logs, the
 * user adds the log here; the desktop app finds its own.
 */
@Component({
  imports: [Button],
  selector: 'app-mouse-panel',
  templateUrl: './mouse-panel.html',
  styleUrl: './mouse-panel.scss',
})
export class MousePanel {
  readonly recording = input.required<Recording>();
  protected readonly logs = inject(MouseLogs);
  protected readonly measures = this.logs.measures(() => this.recording().id);
  protected readonly busy = signal(false);
  protected readonly message = signal<MouseMessage | null>(null);
  protected readonly columns = [...KILL_COLUMNS.map((k) => MEASURES[k].label), 'Corrections'];

  protected readonly shown = computed(() =>
    this.measures.hasValue() ? (this.measures.value() ?? null) : null,
  );
  protected readonly run = computed(() => this.shown()?.run ?? null);
  /** The panel shows where the mode can have a log at all. */
  protected readonly visible = computed(
    () => this.logs.adds || this.logs.logs || this.shown() !== null,
  );
  protected readonly cards = computed(() => {
    const r = this.run();
    return r ? mouseCards(r) : [];
  });
  protected readonly rows = computed(() => {
    const r = this.run();
    return r ? mouseKillRows(r) : [];
  });
  protected readonly source = computed(() => {
    const r = this.run();
    return r ? mouseSource(this.shown()?.file ?? null, r) : '';
  });
  /** The median time between the log's events, ms. */
  protected readonly gapMs = computed(() =>
    Math.round((this.run()?.log.median_interval ?? 0) * 1000),
  );
  protected readonly notes = computed(() => {
    const r = this.run();
    return r ? mouseNotes(r) : '';
  });

  protected async addLog(input: HTMLInputElement): Promise<void> {
    const file = input.files?.[0];
    input.value = '';
    if (!file) return;
    this.busy.set(true);
    this.message.set(null);
    try {
      await this.logs.add(this.recording().id, file);
      this.measures.reload();
    } catch (e) {
      this.message.set({ text: errorMessage(e), failed: true });
    } finally {
      this.busy.set(false);
    }
  }

  protected async forgetLog(): Promise<void> {
    this.busy.set(true);
    try {
      await this.logs.forget(this.recording().id);
      this.measures.reload();
      this.message.set({
        text: 'The log is forgotten here; the file itself is untouched.',
        failed: false,
      });
    } finally {
      this.busy.set(false);
    }
  }

  protected readAgain(): void {
    this.message.set(null);
    this.measures.reload();
  }
}
