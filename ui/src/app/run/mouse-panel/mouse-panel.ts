/**
 * The run page's mouse panel: the run measured from the raw mouse log. In: the recording's measures
 * from the mode's mouse logs (MouseLogs: the core's src/mouse.rs), and a log the user adds. Out:
 * cards with each measure's median, a table of the kills, and notes on the log and the reader's
 * settings.
 */

import { Component, computed, inject, input, signal } from '@angular/core';
import { errorMessage, Recording } from '../../api';
import { DataColumn } from '../../data-table/data-column';
import { DataTable } from '../../data-table/data-table';
import { formatDegrees, formatNumber, formatSpeed } from '../../format';
import { MouseKill, MouseMeasureKey, MouseRun } from '../../mouse-api';
import { MouseLogs } from '../../platform/mouse-logs';

/** How the panel names a measure, what it means, and how its values are written. */
export interface MeasureLabel {
  /** The measure's name on its card and column. */
  label: string;
  /** What it means, for the tooltip. */
  why: string;
  /** A value in words, with its unit. */
  write: (v: number) => string;
}

/** A measure's card: its median, its label, its p10 and p90, and what it means. */
export interface MouseCard {
  /** The measure the card shows. */
  key: MouseMeasureKey;
  /** The median, in words. */
  value: string;
  /** The measure's name. */
  label: string;
  /** The 10th and 90th percentiles, in words. */
  detail: string;
  /** What it means, and how many kills the median is of. */
  why: string;
}

/** One kill's row in the table of kills. */
export interface MouseKillRow {
  /** The kill's number in the run. */
  killNumber: number;
  /** The click's local time, to the millisecond ("12:34:56.789"). */
  at: string;
  /** Each measure in words, in KILL_COLUMNS' order, then the micros. */
  cells: string[];
}

/** What the last action did; failed when it could not be done. */
export interface MouseMessage {
  /** What happened, in words. */
  text: string;
  /** The action failed. */
  failed: boolean;
}

/** Milliseconds in words, whole: "183 ms". */
const ms = (value: number) => `${Math.round(value)} ms`;
/** Degrees a second in words. */
const speed = (value: number) => formatSpeed(value);
/** Degrees in words, to one decimal. */
const degrees = (value: number) => formatDegrees(value, 1);
/** A cell with no value. */
const NONE = '–';

/** Each measure the reader gives: its name, its meaning and how its values are written. */
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
  peak_dps: { label: 'Flick speed', why: 'The highest speed in the flick', write: speed },
  stop_to_click_ms: {
    label: 'Stop to click',
    why: 'From the stop to the click, with any micros after the flick',
    write: ms,
  },
  still_ms: {
    label: 'Confirmation',
    why: 'How long the crosshair sat still on the target before the click; 0 when the click came while it moved',
    write: ms,
  },
  click_dps: {
    label: 'Click on the move',
    why: 'The speed at the click: over the moments just before it',
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

/**
 * The table's columns: the kill, its click's time, then each measure and the micros, their meaning
 * on hover.
 */
const TABLE_COLUMNS: readonly DataColumn<MouseKillRow>[] = [
  { id: 'kill', header: 'Kill', text: (row) => String(row.killNumber), rowHeader: true },
  { id: 'at', header: 'Click at', text: (row) => row.at },
  ...[
    ...KILL_COLUMNS.map((key) => MEASURES[key]),
    { label: 'Micros', why: 'Separate movements after the flick' },
  ].map((measure, index): DataColumn<MouseKillRow> => ({
    id: `measure-${index}`,
    header: measure.label,
    title: measure.why,
    text: (row) => row.cells[index] ?? '',
  })),
];

/** The run's measures as cards: the median of each, with its p10 and p90. */
export function mouseCards(run: MouseRun): MouseCard[] {
  return run.spreads.map((spread) => {
    const measure = MEASURES[spread.key];
    return {
      key: spread.key,
      value: measure.write(spread.median),
      label: measure.label,
      detail: `p10 ${measure.write(spread.p10)} · p90 ${measure.write(spread.p90)}`,
      why: `${measure.why} (median of ${spread.n} kills)`,
    };
  });
}

/** Each kill's row: its number, the click's local time, and its measures. */
export function mouseKillRows(run: MouseRun): MouseKillRow[] {
  return run.kills.map((kill: MouseKill) => ({
    killNumber: kill.n,
    at: kill.press_local.slice(0, 12),
    cells: [
      ...KILL_COLUMNS.map((key) => {
        const value = kill[key];
        return value == null ? NONE : MEASURES[key].write(value);
      }),
      kill.corrections == null ? NONE : String(kill.corrections),
    ],
  }));
}

/**
 * Where the numbers come from: the log, the kills matched with clicks, the misses, and the event
 * rate.
 */
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
  const count = run.kills.length;
  return (
    `Clicked while moving: ${run.moving_clicks} of ${count}; no stop before the click: ${run.no_stop}; ` +
    `with micros: ${run.corrected}. ${formatNumber(run.dpi)} dpi and ${formatNumber(run.cm360)} cm/360 ` +
    `(from ${run.sens_from}); speeds over ${formatNumber(run.window_ms)} ms; moving from ` +
    `${formatNumber(run.start_dps)} °/s, still under ${formatNumber(run.stop_dps)} °/s for ` +
    `${formatNumber(run.hold_ms)} ms.`
  );
}

/**
 * The open recording's run measured from the raw mouse log (src/mouse.rs, python/mouse_read.py):
 * each flick's start, stop and peak speed, and how long the crosshair sat still before each click.
 * Where the browser reads the logs, the user adds the log here; the desktop app finds its own.
 */
@Component({
  imports: [DataTable],
  selector: 'app-mouse-panel',
  templateUrl: './mouse-panel.html',
  styleUrl: './mouse-panel.scss',
})
export class MousePanel {
  /** The open recording. */
  readonly recording = input.required<Recording>();
  /** The mode's mouse logs: the measures, adding and forgetting a log. */
  protected readonly logs = inject(MouseLogs);
  /** The recording's run measured from its log, as the mode reads it. */
  protected readonly measures = this.logs.measures(() => this.recording().id);
  /** Whether a log is being added or forgotten. */
  protected readonly busy = signal(false);
  /** What the last action did; null before one, or after Read again. */
  protected readonly message = signal<MouseMessage | null>(null);
  /** The kills table's columns. */
  protected readonly columns = TABLE_COLUMNS;

  /** The measures once read; null while they load, when reading failed, or with no log. */
  protected readonly shown = computed(() =>
    this.measures.hasValue() ? (this.measures.value() ?? null) : null,
  );
  /** The run's measures; null when there are none (the measures' error says why). */
  protected readonly run = computed(() => this.shown()?.run ?? null);
  /** The panel shows where the mode can have a log at all. */
  protected readonly visible = computed(
    () => this.logs.adds || this.logs.logs || this.shown() !== null,
  );
  /** A card for each measure the kills have. */
  protected readonly cards = computed(() => {
    const run = this.run();
    return run ? mouseCards(run) : [];
  });
  /** A row for each kill. */
  protected readonly rows = computed(() => {
    const run = this.run();
    return run ? mouseKillRows(run) : [];
  });
  /** Where the numbers come from, in words; empty without measures. */
  protected readonly source = computed(() => {
    const run = this.run();
    return run ? mouseSource(this.shown()?.file ?? null, run) : '';
  });
  /** The median time between the log's events, ms. */
  protected readonly gapMs = computed(() =>
    Math.round((this.run()?.log.median_interval ?? 0) * 1000),
  );
  /** The kinds of kill counted and the reader's settings, in words; empty without measures. */
  protected readonly notes = computed(() => {
    const run = this.run();
    return run ? mouseNotes(run) : '';
  });

  /**
   * Reads the log file the user picked and keeps it with the recording, then reads the measures
   * again; a log that cannot be read or does not cover the run shows why.
   */
  protected async addLog(input: HTMLInputElement): Promise<void> {
    const file = input.files?.[0];
    input.value = '';
    if (!file) return;
    this.busy.set(true);
    this.message.set(null);
    try {
      await this.logs.add(this.recording().id, file);
      this.measures.reload();
    } catch (error) {
      this.message.set({ text: errorMessage(error), failed: true });
    } finally {
      this.busy.set(false);
    }
  }

  /** Forgets the log added for the recording; the file on this computer stays as it is. */
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

  /**
   * Reads the measures again, where the app logs the mouse itself and may have logged more since.
   */
  protected readAgain(): void {
    this.message.set(null);
    this.measures.reload();
  }
}
