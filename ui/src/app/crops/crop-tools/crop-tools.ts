import { Component, computed, inject } from '@angular/core';
import { Shape, ShapeKind, ShapeRole } from '../../api';
import { Button } from '../../controls/button';
import { CropDraft } from '../crop-draft';
import { evened, turnedBy } from '../../shapes/shape-geometry';
import {
  changeEach,
  DraftScene,
  inFront,
  joined,
  removed,
  split,
  toggledOccluders,
  withKind,
  withRole,
} from '../crop-scene';

/** A shape button: the kind it gives, flat or 3D (with a third face), and its words. */
interface ShapeChoice {
  kind: ShapeKind;
  deep: boolean;
  name: string;
}

/** A Turn button's step, in degrees. */
const TURN_STEP_DEG = 15;

const SHAPES: readonly ShapeChoice[] = [
  { kind: 'pill', deep: false, name: 'Pill' },
  { kind: 'box', deep: false, name: 'Box' },
  { kind: 'pill', deep: true, name: '3D pill' },
  { kind: 'box', deep: true, name: '3D box' },
];

/** A role button: the role it gives (null: none) and its words. */
interface RoleChoice {
  role: ShapeRole | null;
  name: string;
}

/**
 * The tools of a fix, for the shapes selected on the stage: their kind (and the kind a drag draws), their role, joining
 * them into one target or splitting them, sending them in front or behind, making them hide what is behind them,
 * removing and duplicating them, giving them equal sides, turning them 15° at a time; and Pan, which makes a drag on the
 * stage move the view.
 */
@Component({
  selector: 'app-crop-tools',
  imports: [Button],
  templateUrl: './crop-tools.html',
  styleUrl: './crop-tools.scss',
})
export class CropTools {
  protected readonly draft = inject(CropDraft);
  protected readonly shapes = SHAPES;
  protected readonly TURN_STEP = TURN_STEP_DEG;
  protected readonly roles: readonly RoleChoice[] = [
    { role: null, name: 'None' },
    { role: 'head', name: 'Head' },
    { role: 'body', name: 'Body' },
  ];
  protected readonly chosen = computed<Shape[]>(() => {
    const ids = this.draft.selection();
    return (this.draft.draft()?.shapes ?? []).filter((shape) => ids.includes(shape.id));
  });
  /** The shape a drag draws. */
  protected readonly drawing = computed(() => shapeName(this.draft.kind(), this.draft.deep()));
  /** The shape pressed: the selected shapes' when they share one, else the one a drag draws. */
  protected readonly pressed = computed(() => {
    const names = new Set(this.chosen().map((shape) => shapeName(shape.kind, shape.face !== null)));
    const [only] = names;
    return names.size === 1 ? only : this.drawing();
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

  protected useShape(choice: ShapeChoice): void {
    this.draft.kind.set(choice.kind);
    this.draft.deep.set(choice.deep);
    this.change((scene, ids) => withKind(scene, ids, choice.kind, choice.deep));
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

  protected evenSides(): void {
    this.change((scene, ids) => changeEach(scene, ids, evened));
  }

  /** Turns each selected shape about its middle, clockwise on screen for a positive step. */
  protected turn(degrees: number): void {
    this.change((scene, ids) => changeEach(scene, ids, (shape) => turnedBy(shape, degrees)));
  }

  protected togglePanning(): void {
    this.draft.panning.update((on) => !on);
  }
}

function shapeName(kind: ShapeKind, deep: boolean): string {
  return SHAPES.find((choice) => choice.kind === kind && choice.deep === deep)?.name ?? kind;
}
