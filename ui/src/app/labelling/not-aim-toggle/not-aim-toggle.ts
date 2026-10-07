/**
 * The "Not an aim trainer" button (`NotAimToggle`). In: the recording it marks. Out: the mark,
 * sent through the LabelQueue service to the mode's Labelling.
 */

import { Component, inject, input, signal } from '@angular/core';
import { Recording } from '../../api';
import { Button } from '../../controls/button';
import { LabelQueue } from '../../services/label-queue';

/**
 * The mark for a recording of another game, not an aim trainer: it leaves the labelling queues and what the area
 * finder learns. The button marks the recording, or marks it as an aim trainer again. Marking the labelling queue's
 * recording opens the next.
 */
@Component({
  selector: 'app-not-aim-toggle',
  imports: [Button],
  templateUrl: './not-aim-toggle.html',
})
export class NotAimToggle {
  /** The recording to mark. */
  readonly recording = input.required<Recording>();
  /** The labelling queue, which keeps the mark and moves on. */
  private readonly queue = inject(LabelQueue);
  /** Whether the mark is being saved. */
  protected readonly saving = signal(false);

  /** Marks the recording as another game, or as an aim trainer again. */
  protected async toggleNotAim(): Promise<void> {
    this.saving.set(true);
    try {
      const recording = this.recording();
      await this.queue.setNotAim(recording.id, !recording.not_aim);
    } finally {
      this.saving.set(false);
    }
  }
}
