import { Component, ElementRef, inject, signal, viewChild } from '@angular/core';
import { Button } from '../controls/button';
import { DEVICE_LABELS, Models } from '../services/models';
import { ModelChoice } from './model-choice/model-choice';

/**
 * The model in use, in the top bar; it opens the models side by side (the dialog's body is ModelChoice, loaded on its
 * own), where another one can be picked.
 */
@Component({
  imports: [Button, ModelChoice],
  selector: 'app-model-panel',
  templateUrl: './model-panel.html',
  styleUrl: './model-panel.scss',
})
export class ModelPanel {
  protected readonly models = inject(Models);
  private readonly dialog = viewChild.required<ElementRef<HTMLDialogElement>>('dialog');
  // found by its name, not its class: a class here would load ModelChoice with the panel, not in its @defer block
  private readonly choice = viewChild<ModelChoice>('choice');
  protected readonly deviceLabels = DEVICE_LABELS;
  /**
   * The dialog has been opened: its body loads then and stays, so closing and opening again build nothing (making and
   * tearing down its table cost a frame or two each time). A switch's last word is cleared when it opens.
   */
  protected readonly opened = signal(false);

  protected open(): void {
    this.opened.set(true);
    this.choice()?.clearStatus();
    this.models.list.reload();
    this.dialog().nativeElement.showModal();
  }

  protected close(): void {
    this.dialog().nativeElement.close();
  }
}
