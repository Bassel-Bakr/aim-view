/**
 * The run window panel: where the run starts and ends. In: the review's saved run marks (run.json)
 * and the video's time for "Here". Out: the marks saved through the Review service, which measures
 * the run again with them; the clock format the run page's button uses.
 */

import { Component, computed, inject, linkedSignal, output } from '@angular/core';
import { RunMarks } from '../../api';
import { Review } from '../../services/review';
import { Playback } from '../playback';

/**
 * A time typed as m:ss.s (or seconds), in seconds; null for an empty field, NaN for one that is not
 * a time.
 */
export function parseClock(text: string): number | null {
  const trimmed = text.trim();
  if (!trimmed) return null;
  const parts = trimmed.split(':').map(Number);
  const seconds =
    parts.length === 2 ? 60 * parts[0] + parts[1] : parts.length === 1 ? parts[0] : NaN;
  return Number.isFinite(seconds) && seconds >= 0 ? seconds : NaN;
}

/** Seconds as m:ss.s, to the nearest tenth. */
export function formatClock(seconds: number): string {
  const tenths = Math.round(seconds * 10);
  const minutes = Math.floor(tenths / 600);
  const secondsInMinute = (tenths % 600) / 10;
  return `${minutes}:${secondsInMinute < 10 ? '0' : ''}${secondsInMinute.toFixed(1)}`;
}

/**
 * The user's run window: where the run starts and ends, typed or taken from the video. The review
 * measures only that part; the browser and the desktop app also track only it (with a second either
 * side), skipping menus and waiting around the run. Automatic forgets it.
 */
@Component({
  selector: 'app-run-window',
  templateUrl: './run-window.html',
  styleUrl: './run-window.scss',
})
export class RunWindow {
  /** Fires when the panel should close: Close, or after Save or Automatic. */
  readonly closed = output();
  /** The open recording's review, which keeps the marks and measures with them. */
  protected readonly review = inject(Review);
  /** The video, whose time "Here" takes. */
  private readonly playback = inject(Playback);

  /** The marks kept for the recording, or null when there are none (or they have not loaded). */
  private readonly saved = computed<RunMarks | null>(() =>
    this.review.marks.hasValue() ? (this.review.marks.value() ?? null) : null,
  );
  /** The start field's text, reset to the kept start whenever the marks change. */
  protected readonly start = linkedSignal(() => this.clock(this.saved()?.start));
  /** The end field's text, reset to the kept end whenever the marks change. */
  protected readonly end = linkedSignal(() => this.clock(this.saved()?.end));
  /** Why the times typed cannot be saved; null when they can. */
  protected readonly problem = computed<string | null>(() => {
    const a = parseClock(this.start());
    const b = parseClock(this.end());
    if (Number.isNaN(a) || Number.isNaN(b)) return 'Type a time as m:ss.s, or seconds.';
    if (a === null && b === null) return 'Mark a start, an end, or both.';
    if (a !== null && b !== null && b <= a) return 'The end must come after the start.';
    return null;
  });

  /** A kept mark (seconds) as a field's text; empty when there is none. */
  private clock(seconds: number | null | undefined): string {
    return seconds == null ? '' : formatClock(seconds);
  }

  /** Types the video's time into the start field. */
  protected markStart(): void {
    this.start.set(formatClock(this.playback.time));
  }

  /** Types the video's time into the end field. */
  protected markEnd(): void {
    this.end.set(formatClock(this.playback.time));
  }

  /**
   * Keeps the window; the review is measured again with it (or made again where it tracked less).
   * The kept length stays as it was.
   */
  protected async save(): Promise<void> {
    if (this.problem()) return;
    const start = parseClock(this.start());
    const end = parseClock(this.end());
    await this.review.saveMarks({ start, end, length: this.saved()?.length ?? null });
    this.closed.emit();
  }

  /** Automatic: forgets the marks, so the review finds the run itself, and closes the panel. */
  protected async clear(): Promise<void> {
    await this.review.saveMarks(null);
    this.closed.emit();
  }
}
