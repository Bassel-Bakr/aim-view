/**
 * The labelling queue's bar on the run page (`QueueBar`). In: the open recording and the
 * LabelQueue service. Out: Skip and Stop, sent to the queue.
 */

import { Component, computed, inject, input } from '@angular/core';
import { Recording } from '../../api';
import { LabelQueue } from '../../services/label-queue';

/**
 * The area labelling queue on the run page, while the open recording is the queue's: where it is in the queue, Skip
 * (it leaves the queue for good) and Stop. The areas editor, open on the queue's recording, saves its areas and moves
 * on, and marks it as another game (NotAimToggle).
 */
@Component({
  selector: 'app-queue-bar',
  templateUrl: './queue-bar.html',
  styleUrl: './queue-bar.scss',
})
export class QueueBar {
  /** The recording the run page shows. */
  readonly recording = input.required<Recording>();
  /** The labelling queue. */
  protected readonly queue = inject(LabelQueue);
  /** Whether the bar shows: the recording is the queue's. */
  protected readonly shown = computed(() => this.queue.current() === this.recording().id);

  /** Leaves the recording out of the queue for good, and opens the next. */
  protected skipRecording(): void {
    void this.queue.skip();
  }

  /** Ends the queue; the recording stays open. */
  protected stopQueue(): void {
    this.queue.stop();
  }
}
