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
  readonly recording = input.required<Recording>();
  private readonly queue = inject(LabelQueue);
  protected readonly saving = signal(false);

  protected async toggleNotAim(): Promise<void> {
    this.saving.set(true);
    try {
      const r = this.recording();
      await this.queue.setNotAim(r.id, !r.not_aim);
    } finally {
      this.saving.set(false);
    }
  }
}
