/**
 * The form for an area kind of the user's own: a new kind, or a kind's new name and description.
 * In: the kind to rename (or none) from the area bar. Out: the kind saved through AreaDraft.
 */

import { Component, inject, input, linkedSignal, output, signal } from '@angular/core';
import { form, FormField, maxLength, required } from '@angular/forms/signals';
import { AreaKind } from '../../../api';
import { Button } from '../../../controls/button';
import { AreaDraft } from '../area-draft';

/** A kind's name and what it is, as the form edits them. */
export interface KindFields {
  /** The kind's name, required, up to 40 characters. */
  name: string;
  /** What areas of the kind are, up to 200 characters. */
  about: string;
}

/**
 * A kind of area of the user's own, or a kind's new name and description: areas keep their kind,
 * whatever it is called. A new kind becomes the selected area's.
 */
@Component({
  selector: 'app-kind-form',
  imports: [Button, FormField],
  templateUrl: './kind-form.html',
  styleUrl: './kind-form.scss',
})
export class KindForm {
  /** The kind to rename; null: a new one. */
  readonly kind = input<AreaKind | null>(null);
  /** Fires when the form should close: cancelled, or the kind was kept. */
  readonly closed = output();
  /** The areas editor, which keeps the kind. */
  private readonly draft = inject(AreaDraft);
  /** The fields' values, starting from the kind to rename (empty for a new one). */
  protected readonly model = linkedSignal<KindFields>(() => ({
    name: this.kind()?.name ?? '',
    about: this.kind()?.about ?? '',
  }));
  /** The signal form over the fields, with their rules. */
  protected readonly fields = form(this.model, (path) => {
    required(path.name);
    maxLength(path.name, 40);
    maxLength(path.about, 200);
  });
  /** Whether the kind is being saved, so a second submit waits. */
  protected readonly saving = signal(false);

  /**
   * Submits the form: keeps the kind when the fields are valid, and closes the form once it is kept
   * (else the editor's note says why not).
   */
  protected async saveKind(event: Event): Promise<void> {
    event.preventDefault();
    if (this.fields().invalid() || this.saving()) return;
    this.saving.set(true);
    const { name, about } = this.model();
    const kept = await this.draft.saveKind(this.kind()?.id ?? null, name, about);
    this.saving.set(false);
    if (kept) this.closed.emit();
  }
}
