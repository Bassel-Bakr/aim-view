import { Component, computed, inject, signal } from '@angular/core';
import { errorMessage } from '../../api';
import { Button } from '../../controls/button';
import { FaintCutoffs } from '../../platform/faint-cutoffs';
import { FaintCutoff } from '../../services/faint-cutoff';
import { FaintQueue } from '../../services/faint-queue';
import { Review } from '../../services/review';
import { Playback } from '../playback';
import { faintStrip, STRIP_HEIGHT, STRIP_WIDTH } from './faint-strip';

/** The offsets the cut-off takes. */
const LOWEST = 0.2;
const HIGHEST = 0.6;
const STEP = 0.01;

/**
 * The faint-target cut-off on the run page: leave out the tracks the detector is far less sure of than of this
 * recording's targets (wall seams and tiles), by how far below the recording's level they score. Every track's score
 * is a dot in the strip, the cut a line; clicking a dot shows the track in the video. Submit writes the cut as labels
 * for the detector; in the cut-off queue, Submit and next, and Skip. Where the browser keeps the labels, they download
 * here as the files training reads.
 */
@Component({
  selector: 'app-faint-cutoff',
  imports: [Button],
  templateUrl: './faint-cutoff-panel.html',
  styleUrl: './faint-cutoff-panel.scss',
})
export class FaintCutoffPanel {
  protected readonly faint = inject(FaintCutoff);
  protected readonly queue = inject(FaintQueue);
  protected readonly review = inject(Review);
  private readonly playback = inject(Playback);
  /** The labels kept here (the browser); null where training reads them where they are written. */
  protected readonly labels = inject(FaintCutoffs).labels;
  protected readonly zipping = signal(false);
  /** How many labels are kept here. */
  protected readonly labelsText = computed(() => {
    const c = this.labels?.count();
    if (!c) return '';
    const crops = `${c.crops.toLocaleString()} crop${c.crops === 1 ? '' : 's'}`;
    return `Labels kept in this browser: ${crops} from ${c.recordings} recording${c.recordings === 1 ? '' : 's'}`;
  });
  protected readonly zipFailed = signal<string | null>(null);
  protected readonly lowest = LOWEST;
  protected readonly highest = HIGHEST;
  protected readonly step = STEP;
  protected readonly width = STRIP_WIDTH;
  protected readonly height = STRIP_HEIGHT;
  protected readonly inQueue = computed(() => this.faint.queued());

  protected readonly strip = computed(() => {
    const sc = this.faint.scores();
    return sc && this.faint.has() ? faintStrip(sc, this.faint.cut()) : null;
  });

  /** What the cut-off does as it stands, or why it cannot work. */
  protected readonly info = computed(() => {
    const sc = this.faint.scores();
    if (!sc || !this.faint.has()) {
      return this.faint.allTracks()
        ? "Review again to get the model's scores"
        : 'Reviewing: the scores come with the review';
    }
    const below = this.faint.under().size;
    const saved = this.faint.saved.hasValue() ? this.faint.saved.value() : undefined;
    const labels =
      saved?.submitted === undefined
        ? ''
        : saved.labels == null
          ? ' · submitted, its labels are being written'
          : ` · submitted, ${saved.labels} labels`;
    return (
      `offset ${this.faint.offset().toFixed(2)} · cut ${this.faint.cut().toFixed(2)} ` +
      `(targets ${(sc.level ?? 0).toFixed(2)}) · ${this.faint.on() ? '' : 'would leave out '}` +
      `${below} of ${sc.scores.size} tracks${labels}`
    );
  });

  protected toggleOn(): void {
    this.faint.change({ on: !this.faint.on(), offset: this.faint.offset() });
  }

  protected setOffset(value: number): void {
    this.faint.change({ on: this.faint.on(), offset: value });
  }

  protected toggleScores(): void {
    this.faint.showScores.update((on) => !on);
  }

  /** Shows a track in the video: highlighted, at the middle of the frames it is seen in. */
  protected showTrack(id: number): void {
    const t = this.faint.allTracks();
    const r = this.review.report.hasValue() ? this.review.report.value() : null;
    if (!t || !r) return;
    const seen: number[] = [];
    t.frames.forEach((f, i) => {
      if (f.t.some(([tid]) => tid === id)) seen.push(i);
    });
    if (!seen.length) return;
    this.faint.highlight.set(id);
    this.playback.pause();
    this.playback.seek((seen[Math.floor(seen.length / 2)] + 0.5) / r.fps);
  }

  protected submitCutoff(): void {
    void this.faint.submit();
  }

  protected submitAndNext(): void {
    void this.queue.submitAndNext();
  }

  protected skipRecording(): void {
    void this.queue.skip();
  }

  protected stopQueue(): void {
    this.queue.stop();
  }

  /** Saves the labels to this computer, as the browser saves a download. */
  protected async downloadLabels(): Promise<void> {
    if (!this.labels) return;
    this.zipping.set(true);
    this.zipFailed.set(null);
    try {
      const url = URL.createObjectURL(await this.labels.file());
      const link = document.createElement('a');
      link.href = url;
      link.download = this.labels.fileName;
      link.click();
      setTimeout(() => URL.revokeObjectURL(url));
    } catch (e) {
      this.zipFailed.set(`Could not make the file: ${errorMessage(e)}`);
    } finally {
      this.zipping.set(false);
    }
  }

  protected close(): void {
    this.faint.asked.set(false);
  }
}
