/**
 * The excluded areas editor's bar above the video. In: the editor's state (AreaDraft: areas, kinds,
 * note, busy) and whether a review runs. Out: the user's choices to the editor: a kind for the
 * selected area, a new or renamed kind, Remove, Find areas, Detect fresh, KovOBS's layout, Clear,
 * Save and Cancel; and "Not an aim trainer" to the labelling queue.
 */

import { Component, computed, inject, input, signal } from '@angular/core';
import { AreaKind, Recording } from '../../../api';
import { Button } from '../../../controls/button';
import { NotAimToggle } from '../../../labelling/not-aim-toggle/not-aim-toggle';
import { Review } from '../../../services/review';
import { AreaDraft } from '../area-draft';
import { KindForm } from '../kind-form/kind-form';

/** The kind select's last choice: a kind of the user's own. */
const ADD_KIND = '__add';

/** The kind form: a new kind (kind null), or a kind to rename. */
export interface KindFormState {
  /** The kind to rename; null for a new one. */
  kind: AreaKind | null;
}

/**
 * The excluded areas editor's bar, above the video while the areas are edited over it: what to do,
 * where the areas come from, the selected area's kind (and the user's own kinds), and the editor's
 * actions.
 */
@Component({
  selector: 'app-area-bar',
  imports: [Button, KindForm, NotAimToggle],
  templateUrl: './area-bar.html',
  styleUrl: './area-bar.scss',
})
export class AreaBar {
  /** The recording whose areas are edited, for its "Not an aim trainer" switch. */
  readonly recording = input.required<Recording>();
  /** The editor's state and actions. */
  protected readonly draft = inject(AreaDraft);
  /** The open recording's review: Save waits while it runs. */
  protected readonly review = inject(Review);
  /** The kind select's value for "Add a type", for the template. */
  protected readonly addKind = ADD_KIND;
  /** The kind form, when it is open. */
  protected readonly kindForm = signal<KindFormState | null>(null);
  /** The selected area's kind; null: no area is selected. */
  protected readonly selectedKind = computed<string | null>(() => {
    const at = this.draft.selected();
    return at < 0 ? null : (this.draft.boxes()[at]?.[4] ?? null);
  });
  /** Why Save cannot be pressed now; null when it can. */
  protected readonly saveBlocked = computed<string | null>(() => {
    if (!this.draft.ready()) return 'The areas are being read';
    if (this.review.running()) return 'Wait for the review to end';
    return this.draft.busy() ? 'Busy' : null;
  });

  /** A kind for the selected area, or the kind form for a kind of the user's own. */
  protected pickKind(select: HTMLSelectElement): void {
    if (select.value !== ADD_KIND) {
      this.draft.setKind(select.value);
      return;
    }
    select.value = this.selectedKind() ?? '';
    this.kindForm.set({ kind: null });
  }

  /** The kind form for the selected area's kind: a new name or description. */
  protected editKind(): void {
    const id = this.selectedKind();
    const kind = this.draft.kinds().find((candidate) => candidate.id === id);
    if (kind) this.kindForm.set({ kind });
  }

  /** Closes the kind form. */
  protected closeKindForm(): void {
    this.kindForm.set(null);
  }
}
