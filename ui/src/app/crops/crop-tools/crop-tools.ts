import { Component, computed, inject } from '@angular/core';
import { Shape, ShapeKind, ShapeRole } from '../../api';
import { Button } from '../../controls/button';
import { CropDraft } from '../crop-draft';
import {
  DraftScene,
  inFront,
  joined,
  removed,
  split,
  toggledOccluders,
  withKind,
  withRole,
} from '../crop-scene';

/** A role button: the role it gives (null: none) and its words. */
interface RoleChoice {
  role: ShapeRole | null;
  name: string;
}

/**
 * The tools of a fix, for the shapes selected on the stage: their kind (and the kind a drag draws), their role, joining
 * them into one target or splitting them, sending them in front or behind, making them hide what is behind them,
 * removing and duplicating them; and Pan, which makes a drag on the stage move the view.
 */
@Component({
  selector: 'app-crop-tools',
  imports: [Button],
  templateUrl: './crop-tools.html',
  styleUrl: './crop-tools.scss',
})
export class CropTools {
  protected readonly draft = inject(CropDraft);
  protected readonly kinds: readonly ShapeKind[] = ['pill', 'box'];
  protected readonly kindNames: Readonly<Record<ShapeKind, string>> = { pill: 'Pill', box: 'Box' };
  protected readonly roles: readonly RoleChoice[] = [
    { role: null, name: 'None' },
    { role: 'head', name: 'Head' },
    { role: 'body', name: 'Body' },
  ];
  protected readonly chosen = computed<Shape[]>(() => {
    const ids = this.draft.selection();
    return (this.draft.draft()?.shapes ?? []).filter((shape) => ids.includes(shape.id));
  });
  /** The kind pressed: the selected shapes' when they share one, else the kind a drag draws. */
  protected readonly kind = computed<ShapeKind>(() => {
    const kinds = new Set(this.chosen().map((shape) => shape.kind));
    const [only] = kinds;
    return kinds.size === 1 ? only : this.draft.kind();
  });
  /** The role the selected shapes share; undefined when they differ or none is selected. */
  protected readonly role = computed<ShapeRole | null | undefined>(() => {
    const roles = new Set(this.chosen().map((shape) => shape.role));
    const [only] = roles;
    return roles.size === 1 ? only : undefined;
  });
  protected readonly hiding = computed(() => {
    const occluders = this.draft.draft()?.occluders ?? [];
    return this.chosen().length > 0 && this.chosen().every((shape) => occluders.includes(shape.id));
  });

  private change(change: (scene: DraftScene, ids: readonly string[]) => DraftScene): void {
    const ids = this.draft.selection();
    if (ids.length) this.draft.edit((scene) => change(scene, ids));
  }

  protected useKind(kind: ShapeKind): void {
    this.draft.kind.set(kind);
    this.change((scene, ids) => withKind(scene, ids, kind));
  }

  protected useRole(role: ShapeRole | null): void {
    this.change((scene, ids) => withRole(scene, ids, role));
  }

  protected join(): void {
    this.change(joined);
  }

  protected splitApart(): void {
    this.change(split);
  }

  protected bring(front: boolean): void {
    this.change((scene, ids) => inFront(scene, ids, front));
  }

  protected toggleHiding(): void {
    this.change(toggledOccluders);
  }

  protected remove(): void {
    this.change(removed);
    this.draft.selection.set([]);
  }

  protected togglePanning(): void {
    this.draft.panning.update((on) => !on);
  }
}
