/**
 * The top bar's Label menu (`LabelMenu`): starting the area labelling queue, and the area finder's
 * training data where the browser keeps it. In: the LabelQueue service and its examples store.
 * Out: the queue it starts, and the files it downloads or loads.
 */

import { DecimalPipe } from '@angular/common';
import { Component, computed, ElementRef, inject, signal, viewChild } from '@angular/core';
import { Button } from '../../controls/button';
import { LabelQueue, QueueNote } from '../../services/label-queue';

/**
 * In the top bar: the area labelling queue's button and what the queue last said; where the browser keeps the area
 * finder's training data, its examples too, in a dialog that downloads them as the review server's files and loads
 * those files.
 */
@Component({
  selector: 'app-label-menu',
  imports: [Button, DecimalPipe],
  templateUrl: './label-menu.html',
  styleUrl: './label-menu.scss',
})
export class LabelMenu {
  /** The labelling queue. */
  protected readonly queue = inject(LabelQueue);
  /** The area finder's training data kept here; null where the review server keeps it. */
  protected readonly store = this.queue.examples;
  /** The training data dialog; there only where the browser keeps the data. */
  private readonly dialog = viewChild<ElementRef<HTMLDialogElement>>('dialog');
  /** Whether files are being loaded. */
  protected readonly loading = signal(false);
  /** What the last load did; null before one, or when the dialog opens again. */
  protected readonly status = signal<QueueNote | null>(null);
  /** How much training data is kept; null where the review server keeps it. */
  protected readonly count = computed(() => this.store?.count() ?? null);

  /** Starts the labelling queue. */
  protected startQueue(): void {
    void this.queue.start();
  }

  /** Opens the training data dialog, its last word cleared. */
  protected openExamples(): void {
    this.status.set(null);
    this.dialog()?.nativeElement.showModal();
  }

  /** Closes the training data dialog. */
  protected closeExamples(): void {
    this.dialog()?.nativeElement.close();
  }

  /** Saves one of the files to this computer, as the browser saves a download. */
  protected async downloadFile(name: string): Promise<void> {
    if (!this.store) return;
    const url = URL.createObjectURL(await this.store.file(name));
    const link = document.createElement('a');
    link.href = url;
    link.download = name;
    link.click();
    setTimeout(() => URL.revokeObjectURL(url));
  }

  /** Loads the chosen area_examples.jsonl and area_kinds.json, and says what they held. */
  protected async loadFiles(input: HTMLInputElement): Promise<void> {
    const files = [...(input.files ?? [])];
    input.value = '';
    if (!this.store || !files.length) return;
    this.loading.set(true);
    try {
      const done = await this.store.load(files);
      const read = [
        done.examples ? `${done.examples.toLocaleString()} examples` : '',
        done.kinds ? `${done.kinds} area types` : '',
      ].filter(Boolean);
      const parts = [
        read.length ? `Loaded ${read.join(' and ')}.` : '',
        done.refused.length ? `Could not use ${done.refused.join(', ')}.` : '',
      ];
      this.status.set({ text: parts.filter(Boolean).join(' '), failed: done.refused.length > 0 });
    } finally {
      this.loading.set(false);
    }
  }
}
