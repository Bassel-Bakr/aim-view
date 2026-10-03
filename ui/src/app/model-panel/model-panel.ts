import { Component, computed, ElementRef, inject, signal, viewChild } from '@angular/core';
import { Badge } from '../controls/badge';
import { Button } from '../controls/button';
import { Device, errorMessage } from '../api';
import { DEVICE_LABELS, modelName, Models } from '../services/models';
import { Review } from '../services/review';
import { modelTable } from './model-table';

/** Where the browser runs the detector, as the choice says it. */
const RUNS_ON: Record<Device, string> = { cuda: 'GPU', cpu: 'CPU', wasm: 'CPU', webgpu: 'GPU' };

/**
 * The model in use, in the top bar; it opens the models side by side (what each does best, its checks and speeds),
 * where another one can be picked. New reviews use the pick; a recording's reviews by other models stay.
 */
@Component({
  imports: [Button, Badge],
  selector: 'app-model-panel',
  templateUrl: './model-panel.html',
  styleUrl: './model-panel.scss',
})
export class ModelPanel {
  protected readonly models = inject(Models);
  private readonly review = inject(Review);
  private readonly dialog = viewChild.required<ElementRef<HTMLDialogElement>>('dialog');
  protected readonly deviceLabels = DEVICE_LABELS;
  protected readonly runsOn = RUNS_ON;
  protected readonly table = computed(() => {
    const list = this.models.current();
    return list ? modelTable(list) : null;
  });
  protected readonly switching = signal(false);
  protected readonly status = signal('');

  protected useDevice(device: Device): void {
    void this.models.useDevice(device);
  }

  protected useBatch(batch: number): void {
    void this.models.useBatch(batch);
  }

  protected open(): void {
    this.status.set('');
    this.models.list.reload();
    this.dialog().nativeElement.showModal();
  }

  protected close(): void {
    this.dialog().nativeElement.close();
  }

  /** Picks the model new reviews use; the open recording shows its review by that model, when it has one. */
  protected async use(name: string): Promise<void> {
    this.switching.set(true);
    this.status.set(`Loading ${modelName(name)}…`);
    try {
      await this.models.pick(name);
    } catch (e) {
      this.status.set(`Could not switch: ${errorMessage(e)}`);
      return;
    } finally {
      this.switching.set(false);
    }
    this.status.set(`Now using ${modelName(name)}. New reviews use it.`);
    this.review.report.reload();
  }
}
