import { Component, computed, inject, linkedSignal, output } from '@angular/core';
import { RunMarks } from '../../api';
import { Button } from '../../controls/button';
import { Review } from '../../services/review';
import { Playback } from '../playback';

/** A time typed as m:ss.s (or seconds); null for an empty field, NaN for one that is not a time. */
export function parseClock(text: string): number | null {
  const trimmed = text.trim();
  if (!trimmed) return null;
  const parts = trimmed.split(':').map(Number);
  const seconds =
    parts.length === 2 ? 60 * parts[0] + parts[1] : parts.length === 1 ? parts[0] : NaN;
  return Number.isFinite(seconds) && seconds >= 0 ? seconds : NaN;
}

/** Seconds as m:ss.s. */
export function formatClock(seconds: number): string {
  const tenths = Math.round(seconds * 10);
  const minutes = Math.floor(tenths / 600);
  const secondsInMinute = (tenths % 600) / 10;
  return `${minutes}:${secondsInMinute < 10 ? '0' : ''}${secondsInMinute.toFixed(1)}`;
}

/**
 * The user's run window: where the run starts and ends, typed or taken from the video. The review measures only that
 * part; the browser and the desktop app also track only it (with a second either side), skipping menus and waiting
 * around the run. Automatic forgets it.
 */
@Component({
  selector: 'app-run-window',
  imports: [Button],
  templateUrl: './run-window.html',
  styleUrl: './run-window.scss',
})
export class RunWindow {
  readonly closed = output();
  protected readonly review = inject(Review);
  private readonly playback = inject(Playback);

  private readonly saved = computed<RunMarks | null>(() =>
    this.review.marks.hasValue() ? (this.review.marks.value() ?? null) : null,
  );
  protected readonly start = linkedSignal(() => this.clock(this.saved()?.start));
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

  private clock(seconds: number | null | undefined): string {
    return seconds == null ? '' : formatClock(seconds);
  }

  protected markStart(): void {
    this.start.set(formatClock(this.playback.time));
  }

  protected markEnd(): void {
    this.end.set(formatClock(this.playback.time));
  }

  /** Keeps the window; the review is measured again with it (or made again where it tracked less). */
  protected async save(): Promise<void> {
    if (this.problem()) return;
    const start = parseClock(this.start());
    const end = parseClock(this.end());
    await this.review.saveMarks({ start, end, length: this.saved()?.length ?? null });
    this.closed.emit();
  }

  protected async clear(): Promise<void> {
    await this.review.saveMarks(null);
    this.closed.emit();
  }
}
