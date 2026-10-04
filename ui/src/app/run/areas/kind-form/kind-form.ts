import { Component, inject, input, linkedSignal, output, signal } from '@angular/core';
import { form, FormField, maxLength, required } from '@angular/forms/signals';
import { AreaKind } from '../../../api';
import { Button } from '../../../controls/button';
import { AreaDraft } from '../area-draft';

/** A kind's name and what it is, as the form edits them. */
export interface KindFields {
  name: string;
  about: string;
}

/**
 * A kind of area of the user's own, or a kind's new name and description: areas keep their kind, whatever it is
 * called. A new kind becomes the selected area's.
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
  readonly closed = output();
  private readonly draft = inject(AreaDraft);
  protected readonly model = linkedSignal<KindFields>(() => ({
    name: this.kind()?.name ?? '',
    about: this.kind()?.about ?? '',
  }));
  protected readonly fields = form(this.model, (path) => {
    required(path.name);
    maxLength(path.name, 40);
    maxLength(path.about, 200);
  });
  protected readonly saving = signal(false);

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
