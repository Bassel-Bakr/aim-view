/**
 * The run page's Cut-off panel. In: the FaintCutoff service (the scores, the setting, the tracks),
 * the cut-off queue, the review's tracks and report, and the labels the browser keeps. Out: the
 * user's setting (on, offset) to FaintCutoff, a track shown in the video, submitted cut-offs, the
 * queue's moves, and the labels' download.
 */

import { Component, computed, inject, signal } from '@angular/core';
import { errorMessage } from '../../api';
import { Button } from '../../controls/button';
import { FaintCutoffs } from '../../platform/faint-cutoffs';
import { FaintCutoff } from '../../services/faint-cutoff';
import { FaintQueue } from '../../services/faint-queue';
import { Review } from '../../services/review';
import { Playback } from '../playback';
import { faintStrip, STRIP_HEIGHT, STRIP_WIDTH } from './faint-strip';

/** The lowest offset the slider takes, in detector score. */
const LOWEST = 0.2;
/** The highest offset the slider takes, in detector score. */
const HIGHEST = 0.6;
/** The slider's step, in detector score. */
const STEP = 0.01;

/**
 * The faint-target cut-off on the run page: leave out the tracks the detector is far less sure of
 * than of this recording's targets (wall seams and tiles), by how far below the recording's level
 * they score. Every track's score is a dot in the strip, the cut a line; clicking a dot shows the
 * track in the video. Submit writes the cut as labels for the detector; in the cut-off queue,
 * Submit and next, and Skip. Where the browser keeps the labels, they download here as the files
 * training reads.
 */
@Component({
  selector: 'app-faint-cutoff',
  imports: [Button],
  templateUrl: './faint-cutoff-panel.html',
  styleUrl: './faint-cutoff-panel.scss',
})
export class FaintCutoffPanel {
  /** The open recording's cut-off: its scores, setting and the tracks it leaves out. */
  protected readonly faint = inject(FaintCutoff);
  /** The cut-off queue: Submit and next, Skip, Stop. */
  protected readonly queue = inject(FaintQueue);
  /** The open recording's review, for its fps when a track is shown. */
  protected readonly review = inject(Review);
  /** The video, which a shown track seeks to. */
  private readonly playback = inject(Playback);
  /** The labels kept here (the browser); null where training reads them where they are written. */
  protected readonly labels = inject(FaintCutoffs).labels;
  /** Whether the labels' zip is being made for download. */
  protected readonly zipping = signal(false);
  /** How many labels are kept here, in words; empty when none are. */
  protected readonly labelsText = computed(() => {
    const count = this.labels?.count();
    if (!count) return '';
    const crops = `${count.crops.toLocaleString()} crop${count.crops === 1 ? '' : 's'}`;
    return `Labels kept in this browser: ${crops} from ${count.recordings} recording${count.recordings === 1 ? '' : 's'}`;
  });
  /** Why the labels' zip could not be made; null when it could (or was not tried). */
  protected readonly zipFailed = signal<string | null>(null);
  /** The slider's lowest offset, for the template. */
  protected readonly lowest = LOWEST;
  /** The slider's highest offset, for the template. */
  protected readonly highest = HIGHEST;
  /** The slider's step, for the template. */
  protected readonly step = STEP;
  /** The strip's view box width, for the template. */
  protected readonly width = STRIP_WIDTH;
  /** The strip's view box height, for the template. */
  protected readonly height = STRIP_HEIGHT;
  /** The open recording is the cut-off queue's: the queue's buttons show. */
  protected readonly inQueue = computed(() => this.faint.queued());

  /** The strip of track scores and the cut; null until the review has the detector's scores. */
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

  /** Turns the cut-off on or off at the same offset. */
  protected toggleOn(): void {
    this.faint.change({ on: !this.faint.on(), offset: this.faint.offset() });
  }

  /** Sets the offset below the recording's level, leaving the cut-off on or off as it is. */
  protected setOffset(value: number): void {
    this.faint.change({ on: this.faint.on(), offset: value });
  }

  /** Shows or hides every track's score beside it on the video. */
  protected toggleScores(): void {
    this.faint.showScores.update((on) => !on);
  }

  /** Shows a track in the video: highlighted, at the middle of the frames it is seen in. */
  protected showTrack(id: number): void {
    const tracks = this.faint.allTracks();
    const report = this.review.report.hasValue() ? this.review.report.value() : null;
    if (!tracks || !report) return;
    const seen: number[] = [];
    tracks.frames.forEach((trackFrame, i) => {
      if (trackFrame.t.some(([tid]) => tid === id)) seen.push(i);
    });
    if (!seen.length) return;
    this.faint.highlight.set(id);
    this.playback.pause();
    this.playback.seek((seen[Math.floor(seen.length / 2)] + 0.5) / report.fps);
  }

  /** Submit: keeps the cut-off on and writes it as labels for the detector. */
  protected submitCutoff(): void {
    void this.faint.submit();
  }

  /** Submits the cut-off, then opens the queue's next recording. */
  protected submitAndNext(): void {
    void this.queue.submitAndNext();
  }

  /** Leaves this recording out of the queue from now on, and opens the next. */
  protected skipRecording(): void {
    void this.queue.skip();
  }

  /** Ends the cut-off queue; the recording stays open. */
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
    } catch (error) {
      this.zipFailed.set(`Could not make the file: ${errorMessage(error)}`);
    } finally {
      this.zipping.set(false);
    }
  }

  /** Closes the panel; it stays while the cut-off is on or the queue is on this recording. */
  protected close(): void {
    this.faint.asked.set(false);
  }
}
