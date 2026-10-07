/**
 * The top bar's Cut-off menu (`CutoffMenu`). In: the FaintQueue service. Out: the queue it starts.
 */

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
  /** The cut-off queue. */
  protected readonly queue = inject(FaintQueue);

  /** Starts the cut-off queue. */
  protected startQueue(): void {
    void this.queue.start();
  }
}
