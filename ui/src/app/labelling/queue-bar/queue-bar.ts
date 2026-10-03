import { Component, computed, inject, input } from '@angular/core';
import { Recording } from '../../api';
import { Button } from '../../controls/button';
import { LabelQueue } from '../../services/label-queue';

/**
 * The area labelling queue on the run page, while the open recording is the queue's: where it is in the queue, Skip
 * (it leaves the queue for good) and Stop. The areas editor, open on the queue's recording, saves its areas and moves
 * on, and marks it as another game (NotAimToggle).
 */
@Component({
  selector: 'app-queue-bar',
  imports: [Button],
  templateUrl: './queue-bar.html',
  styleUrl: './queue-bar.scss',
})
export class QueueBar {
  readonly recording = input.required<Recording>();
  protected readonly queue = inject(LabelQueue);
  protected readonly shown = computed(() => this.queue.current() === this.recording().id);

  protected skipRecording(): void {
    void this.queue.skip();
  }

  protected stopQueue(): void {
    this.queue.stop();
  }
}
