import { HttpClient } from '@angular/common/http';
import { Component, computed, ElementRef, inject, signal, viewChild } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { errorMessage, ModelList } from '../api';
import { modelName, Models } from '../services/models';
import { Review } from '../services/review';
import { modelTable } from './model-table';
import { badge, button, note } from '@themes/controls.styles';
import { modelPanelStyles } from '@themes/model-panel.styles';
import { slotClasses } from '@themes/slot-classes';

/**
 * The model in use, in the top bar; it opens the models side by side (what each does best, its checks and speeds),
 * where another one can be picked. New reviews use the pick; a recording's reviews by other models stay.
 */
@Component({
  selector: 'app-model-panel',
  templateUrl: './model-panel.html',
})
export class ModelPanel {
  protected readonly models = inject(Models);
  private readonly http = inject(HttpClient);
  private readonly review = inject(Review);
  private readonly dialog = viewChild.required<ElementRef<HTMLDialogElement>>('dialog');
  protected readonly ui = slotClasses(modelPanelStyles());
  protected readonly button = button();
  protected readonly note = note();
  protected readonly badge = badge();
  protected readonly goodBadge = badge({ tone: 'good' });
  protected readonly table = computed(() =>
    this.models.list.hasValue() ? modelTable(this.models.list.value()) : null,
  );
  protected readonly switching = signal(false);
  protected readonly status = signal('');

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
      const list = await firstValueFrom(
        this.http.post<ModelList>('/api/model', null, { params: { name } }),
      );
      this.models.list.set(list);
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
