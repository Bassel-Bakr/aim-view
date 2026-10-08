/**
 * The Crops page's tools for a fix (`CropTools`): buttons that change the shapes selected on the
 * stage. In: the CropDraft state (its scene and selection). Out: edits to the draft's scene
 * (crop-scene.ts does each one) and the drawing choices a drag on the stage uses.
 */

import { Component, computed, inject } from '@angular/core';
import { Shape, ShapeKind, ShapeRole } from '../../api';
import { CropDraft } from '../crop-draft';
import { evened, turnedBy } from '../../shapes/shape-geometry';
import { showing, SideShown } from '../../shapes/solid-geometry';
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
  /** The kind of shape it gives. */
  kind: ShapeKind;
  /** Whether the shape is 3D. */
  deep: boolean;
  /** The button's words. */
  name: string;
}

/** A 3D turn button: the side it turns toward the camera, and its words. */
interface SideChoice {
  /** The side it turns toward the camera. */
  side: SideShown;
  /** The button's words. */
  name: string;
}

/** The 3D turn buttons, in the page's order. */
const SIDES_SHOWN: readonly SideChoice[] = [
  { side: 'top', name: 'Show top' },
  { side: 'bottom', name: 'Show bottom' },
  { side: 'left', name: 'Show left' },
  { side: 'right', name: 'Show right' },
];

/** A Turn button's step, in degrees. */
const TURN_STEP_DEG = 15;

/** The shape buttons, in the page's order. */
const SHAPES: readonly ShapeChoice[] = [
  { kind: 'pill', deep: false, name: 'Pill' },
  { kind: 'box', deep: false, name: 'Box' },
  { kind: 'pill', deep: true, name: '3D pill' },
  { kind: 'box', deep: true, name: '3D box' },
];

/** A role button: the role it gives (null: none) and its words. */
interface RoleChoice {
  /** The role it gives; null for none. */
  role: ShapeRole | null;
  /** The button's words. */
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
  templateUrl: './crop-tools.html',
  styleUrl: './crop-tools.scss',
})
export class CropTools {
  /** The page's state: the scene being fixed, the selection and the drawing choices. */
  protected readonly draft = inject(CropDraft);
  /** The shape buttons. */
  protected readonly shapes = SHAPES;
  /** A Turn button's step, in degrees, for the template. */
  protected readonly TURN_STEP = TURN_STEP_DEG;
  /** The 3D turn buttons. */
  protected readonly sidesShown = SIDES_SHOWN;
  /** The role buttons. */
  protected readonly roles: readonly RoleChoice[] = [
    { role: null, name: 'None' },
    { role: 'head', name: 'Head' },
    { role: 'body', name: 'Body' },
  ];
  /** The selected shapes of the scene being fixed. */
  protected readonly chosen = computed<Shape[]>(() => {
    const ids = this.draft.selection();
    return (this.draft.draft()?.shapes ?? []).filter((shape) => ids.includes(shape.id));
  });
  /** The shape a drag draws. */
  protected readonly drawing = computed(() => shapeName(this.draft.kind(), this.draft.deep()));
  /** The shape pressed: the selected shapes' when they share one, else the one a drag draws. */
  protected readonly pressed = computed(() => {
    const names = new Set(
      this.chosen().map((shape) =>
        shapeName(shape.kind, shape.solid !== null || shape.face !== null),
      ),
    );
    const [only] = names;
    return names.size === 1 ? only : this.drawing();
  });
  /** The role the selected shapes share; undefined when they differ or none is selected. */
  protected readonly role = computed<ShapeRole | null | undefined>(() => {
    const roles = new Set(this.chosen().map((shape) => shape.role));
    const [only] = roles;
    return roles.size === 1 ? only : undefined;
  });
  /** Whether every selected shape is an occluder; false with none selected. */
  protected readonly hiding = computed(() => {
    const occluders = this.draft.draft()?.occluders ?? [];
    return this.chosen().length > 0 && this.chosen().every((shape) => occluders.includes(shape.id));
  });

  /** Applies an edit to the selected shapes; nothing when none is selected. */
  private change(change: (scene: DraftScene, ids: readonly string[]) => DraftScene): void {
    const ids = this.draft.selection();
    if (ids.length) this.draft.edit((scene) => change(scene, ids));
  }

  /** Makes the selected shapes this kind, and makes it the kind a drag draws. */
  protected useShape(choice: ShapeChoice): void {
    this.draft.kind.set(choice.kind);
    this.draft.deep.set(choice.deep);
    this.change((scene, ids) => withKind(scene, ids, choice.kind, choice.deep));
  }

  /** Gives the selected shapes a role (null: none). */
  protected useRole(role: ShapeRole | null): void {
    this.change((scene, ids) => withRole(scene, ids, role));
  }

  /** Joins the selected shapes into one target. */
  protected join(): void {
    this.change(joined);
  }

  /** Makes each selected shape a target of its own again. */
  protected splitApart(): void {
    this.change(split);
  }

  /** Brings the selected shapes in front of the others, or sends them behind. */
  protected bring(front: boolean): void {
    this.change((scene, ids) => inFront(scene, ids, front));
  }

  /** Makes the selected shapes occluders, or targets again when all of them are. */
  protected toggleHiding(): void {
    this.change(toggledOccluders);
  }

  /** Removes the selected shapes (a model box is crossed out) and clears the selection. */
  protected remove(): void {
    this.change(removed);
    this.draft.selection.set([]);
  }

  /** Gives each selected shape equal sides (a circle or a square). */
  protected evenSides(): void {
    this.change((scene, ids) => changeEach(scene, ids, evened));
  }

  /** Turns each selected shape about its middle, clockwise on screen for a positive step. */
  protected turn(degrees: number): void {
    this.change((scene, ids) => changeEach(scene, ids, (shape) => turnedBy(shape, degrees)));
  }

  /** A 3D shape among the selected ones: the 3D turn buttons show. */
  protected readonly solidChosen = computed(() =>
    this.chosen().some((shape) => shape.solid !== null && shape.points === null),
  );
  /** A box placed by hand among the selected ones: Back to a box shows. */
  protected readonly placedChosen = computed(() =>
    this.chosen().some((shape) => shape.points !== null),
  );

  /** Turns each selected 3D shape so a side comes toward the camera, a step at a time. */
  protected show(side: SideShown): void {
    this.change((scene, ids) =>
      changeEach(scene, ids, (shape) =>
        shape.solid && !shape.points ? showing(shape, shape.solid, side, TURN_STEP_DEG) : shape,
      ),
    );
  }

  /** Turns each selected 3D shape to face the camera squarely, its turn on screen kept. */
  protected faceCamera(): void {
    this.change((scene, ids) =>
      changeEach(scene, ids, (shape) =>
        shape.solid ? { ...shape, solid: { ...shape.solid, tip: 0, swing: 0 } } : shape,
      ),
    );
  }

  /** Undoes the selected boxes' placed corners: each is its box again, flat or solid as it was. */
  protected unplace(): void {
    this.change((scene, ids) => changeEach(scene, ids, (shape) => ({ ...shape, points: null })));
  }

  /** Turns Mirror on or off: a side dragged moves its opposite side too. */
  protected toggleMirroring(): void {
    this.draft.mirroring.update((on) => !on);
  }

  /** Turns Pan on or off: a drag on the stage moves the view. */
  protected togglePanning(): void {
    this.draft.panning.update((on) => !on);
  }
}

/** A shape's button words ("3D pill"); the kind itself when no button gives it. */
function shapeName(kind: ShapeKind, deep: boolean): string {
  return SHAPES.find((choice) => choice.kind === kind && choice.deep === deep)?.name ?? kind;
}
