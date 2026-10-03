import { Component, inject } from '@angular/core';
import { Button } from '../controls/button';
import { FaintQueue } from '../services/faint-queue';

/** In the top bar: the cut-off queue's button, and what the queue last said. */
@Component({
  selector: 'app-cutoff-menu',
  imports: [Button],
  templateUrl: './cutoff-menu.html',
  styleUrl: './cutoff-menu.scss',
})
export class CutoffMenu {
  protected readonly queue = inject(FaintQueue);

  protected startQueue(): void {
    void this.queue.start();
  }
}
