/**
 * The edits a fix makes to a crop's scene, as pure functions: a scene from a crop and its answer
 * or a suggestion, each edit (draw, copy, scale, join, hide, cross out), and the answer a scene
 * gives back. In: the crop's model boxes and its answer (api.ts). Out: crop-draft.ts, which holds
 * the scene, and the crop stage and tools that edit it.
 */

import {
  CropAnswer,
  CropBox,
  CropEntry,
  CropVertex,
  CropVerdict,
  FaceOffset,
  Shape,
  ShapeKind,
  ShapeRole,
  Solid,
} from '../api';
import { START_SWING_DEG, START_TIP_DEG } from '../shapes/solid-geometry';
import { boxAround, CropPoint, scaled } from '../shapes/shape-geometry';
import { CropFix } from './crop-lessons';

/**
 * A crop's shapes as the page edits them (the scene of src/shapes.rs, and the model boxes crossed out), made from a
 * crop and its answer or suggestion, and turned back into an answer. Pure: crop-draft.ts holds the state.
 */
export interface DraftScene {
  /** Every shape, the model's boxes (ids m0, m1...) and the ones drawn. */
  shapes: Shape[];
  /** Shapes joined into one target, as lists of shape ids; a shape in none is a target alone. */
  targets: string[][];
  /** The ids of the shapes that hide what is behind them instead of being targets. */
  occluders: string[];
  /** The model boxes crossed out: no target there. */
  crossed: number[];
}

/** A tapped point's shape is this big when the crop has no box to take a size from (crop pixels). */
const POINT_PX = 8;

/** A pill standing for a box. */
function pill(id: string, box: CropBox, model: number | null): Shape {
  return {
    id,
    kind: 'pill',
    box: [...box],
    angle: 0,
    face: null,
    solid: null,
    points: null,
    depth: 0,
    role: null,
    model,
  };
}

/** The median of the crop's model boxes' sides, for a tapped point; POINT_PX when it has none. */
function pointSide(crop: CropEntry): number {
  const sides = crop.boxes
    .map(([, , width, height]) => Math.max(width, height))
    .sort((a, b) => a - b);
  return sides.length ? sides[sides.length >> 1] : POINT_PX;
}

/** A crop's scene from the claude.ai page's fields (or a suggestion's): its boxes as pills, less those crossed out. */
export function sceneOfFix(crop: CropEntry, fix: CropFix | null): DraftScene {
  const remove = fix?.remove ?? [];
  const shapes = crop.boxes
    .map((box, index) => pill(`m${index}`, fix?.edit[String(index)] ?? box, index))
    .filter((shape) => !remove.includes(shape.model ?? -1));
  (fix?.add ?? []).forEach((box, index) => shapes.push(pill(`a${index}`, box, null)));
  return { shapes, targets: [], occluders: [], crossed: [...remove] };
}

/** A crop's scene from its answer: the scene drawn on this page, else made from the claude.ai page's fields. */
export function sceneOfAnswer(crop: CropEntry, answer: CropAnswer): DraftScene {
  if (answer.scene) {
    const { shapes, targets, occluders } = answer.scene;
    return {
      shapes: shapes.map((shape) => ({ ...shape })),
      targets,
      occluders,
      crossed: [...answer.remove],
    };
  }
  const boxes = answer.add.filter((mark) => mark.length === 4) as CropBox[];
  const scene = sceneOfFix(crop, { remove: answer.remove, edit: answer.edit, add: boxes });
  const side = pointSide(crop);
  answer.add
    .filter((mark) => mark.length === 2)
    .forEach(([x, y], index) => scene.shapes.push(pill(`p${index}`, [x, y, side, side], null)));
  return scene;
}

/** A value to a tenth of a pixel: finer is noise from the finger and the turning. */
const tenth = (value: number) => Math.round(value * 10) / 10;

/** A shape with its box, third face, solid and placed vertices to a tenth of a pixel (a degree for its turns). */
function tidied(shape: Shape): Shape {
  const [cx, cy, width, height] = shape.box;
  const face: FaceOffset | null = shape.face && [tenth(shape.face[0]), tenth(shape.face[1])];
  const solid: Solid | null = shape.solid && {
    thickness: tenth(shape.solid.thickness),
    tip: tenth(shape.solid.tip),
    swing: tenth(shape.solid.swing),
  };
  const points: CropVertex[] | null =
    shape.points && shape.points.map(([x, y]): CropVertex => [tenth(x), tenth(y)]);
  return {
    ...shape,
    box: [tenth(cx), tenth(cy), tenth(width), tenth(height)],
    angle: tenth(shape.angle),
    face,
    solid,
    points,
  };
}

/**
 * An answer from a scene: the scene itself, and the claude.ai page's fields made from it (the model boxes crossed
 * out, the ones moved or resized, and the box round each drawn shape that is no occluder), which the page's
 * suggestions learn from.
 */
export function answerOf(
  crop: CropEntry,
  scene: DraftScene,
  verdict: CropVerdict,
  suggested: boolean,
): CropAnswer {
  const { targets, occluders } = scene;
  const shapes = scene.shapes.map(tidied);
  const edit: Record<string, CropBox> = {};
  for (const shape of shapes) {
    if (shape.model === null) continue;
    const model = crop.boxes[shape.model];
    if (model && shape.box.some((value, i) => Math.abs(value - model[i]) > 1e-6))
      edit[String(shape.model)] = shape.box;
  }
  const add = shapes
    .filter((shape) => shape.model === null && !occluders.includes(shape.id))
    .map((shape) => boxAround([shape]).map(tenth) as CropBox);
  return {
    verdict,
    set: crop.set,
    file: crop.file,
    at: Date.now(),
    remove: [...scene.crossed].sort((a, b) => a - b),
    edit,
    add,
    ...(suggested ? { suggested: true } : {}),
    scene: { shapes, targets, occluders },
  };
}

/** An id no shape of the scene has yet. */
export function freshId(scene: DraftScene): string {
  const used = new Set(scene.shapes.map((shape) => shape.id));
  let next = scene.shapes.length;
  while (used.has(`s${next}`)) next += 1;
  return `s${next}`;
}

/** A new 3D shape's solid: as thick as its short side, tipped and swung so its top and right side show. */
export function defaultSolid(box: CropBox): Solid {
  return { thickness: Math.min(box[2], box[3]), tip: START_TIP_DEG, swing: START_SWING_DEG };
}

/** The scene with a new shape on top of the others; deep: a 3D one, solid. */
export function withShape(
  scene: DraftScene,
  kind: ShapeKind,
  box: CropBox,
  deep = false,
): DraftScene {
  const depth = Math.max(0, ...scene.shapes.map((shape) => shape.depth));
  const solid = deep ? defaultSolid(box) : null;
  const shape: Shape = { ...pill(freshId(scene), box, null), kind, solid, depth };
  return { ...scene, shapes: [...scene.shapes, shape] };
}

/**
 * The scene with a shape drawn on top: it replaces the model shapes whose middle it covers (a box too small or off,
 * or a target found in pieces), which are crossed out, as the claude.ai page did. A tap on a cross brings one back.
 */
export function withDrawn(
  scene: DraftScene,
  kind: ShapeKind,
  box: CropBox,
  deep = false,
): DraftScene {
  const [cx, cy, width, height] = box;
  const covered = scene.shapes
    .filter(
      ({ model, box: [x, y] }) =>
        model !== null && Math.abs(x - cx) <= width / 2 && Math.abs(y - cy) <= height / 2,
    )
    .map((shape) => shape.id);
  return removed(withShape(scene, kind, box, deep), covered);
}

/** A scene with shapes copied, and the copies' ids. */
export interface SceneCopy {
  /** The scene with the copies added. */
  scene: DraftScene;
  /** The copies' ids, in the order of the shapes they copy. */
  copies: string[];
}

/**
 * The selected shapes copied, moved by (dx, dy): each copy keeps its kind, size, angle, face, role and depth, and is a
 * drawn shape. Copies of shapes joined together are joined into a new target, and copies of occluders hide too.
 */
export function duplicated(
  scene: DraftScene,
  ids: readonly string[],
  [dx, dy]: CropPoint,
): SceneCopy {
  const renamed = new Map<string, string>();
  let shapes = scene.shapes;
  for (const shape of scene.shapes.filter((one) => ids.includes(one.id))) {
    const id = freshId({ ...scene, shapes });
    const [cx, cy, width, height] = shape.box;
    renamed.set(shape.id, id);
    shapes = [...shapes, { ...shape, id, model: null, box: [cx + dx, cy + dy, width, height] }];
  }
  const copyOf = (id: string) => renamed.get(id) ?? id;
  const targets = scene.targets
    .map((group) => group.filter((id) => renamed.has(id)).map(copyOf))
    .filter((group) => group.length > 1);
  const occluders = scene.occluders.filter((id) => renamed.has(id)).map(copyOf);
  return {
    scene: {
      ...scene,
      shapes,
      targets: [...scene.targets, ...targets],
      occluders: [...scene.occluders, ...occluders],
    },
    copies: [...renamed.values()],
  };
}

/** The selected shapes scaled together by a factor, about the middle of the box round them. */
export function resizedAll(scene: DraftScene, ids: readonly string[], factor: number): DraftScene {
  const chosen = scene.shapes.filter((shape) => ids.includes(shape.id));
  if (!chosen.length) return scene;
  const [cx, cy] = boxAround(chosen);
  return changeEach(scene, ids, (shape) => scaled(shape, factor, [cx, cy]));
}

/** The scene with one shape replaced (same id). */
export function withChanged(scene: DraftScene, changed: Shape): DraftScene {
  return {
    ...scene,
    shapes: scene.shapes.map((shape) => (shape.id === changed.id ? changed : shape)),
  };
}

/** The scene with every selected shape changed by `change`. */
export function changeEach(
  scene: DraftScene,
  ids: readonly string[],
  change: (shape: Shape) => Shape,
): DraftScene {
  return {
    ...scene,
    shapes: scene.shapes.map((shape) => (ids.includes(shape.id) ? change(shape) : shape)),
  };
}

/** The selected shapes made one kind, flat or 3D (deep: solid, keeping its tumble, or given the starting one). */
export function withKind(
  scene: DraftScene,
  ids: readonly string[],
  kind: ShapeKind,
  deep = false,
): DraftScene {
  return changeEach(scene, ids, (shape) => ({
    ...shape,
    kind,
    face: null,
    solid: deep ? (shape.solid ?? defaultSolid(shape.box)) : null,
    points: null,
  }));
}

/** The selected shapes given a role (null: none). */
export function withRole(
  scene: DraftScene,
  ids: readonly string[],
  role: ShapeRole | null,
): DraftScene {
  return changeEach(scene, ids, (shape) => ({ ...shape, role }));
}

/** The scene with the selected shapes out of any target group and of the occluders. */
function loosened(scene: DraftScene, ids: readonly string[]): DraftScene {
  const targets = scene.targets
    .map((group) => group.filter((id) => !ids.includes(id)))
    .filter((group) => group.length > 1);
  return { ...scene, targets, occluders: scene.occluders.filter((id) => !ids.includes(id)) };
}

/** The selected shapes joined into one target. */
export function joined(scene: DraftScene, ids: readonly string[]): DraftScene {
  const loose = loosened(scene, ids);
  return ids.length > 1 ? { ...loose, targets: [...loose.targets, [...ids]] } : loose;
}

/** The selected shapes each a target of its own again. */
export function split(scene: DraftScene, ids: readonly string[]): DraftScene {
  return loosened(scene, ids);
}

/** The selected shapes brought in front of every other shape, or sent behind them. */
export function inFront(scene: DraftScene, ids: readonly string[], front: boolean): DraftScene {
  const others = scene.shapes
    .filter((shape) => !ids.includes(shape.id))
    .map((shape) => shape.depth);
  const depth = front ? Math.max(0, ...others) + 1 : Math.min(0, ...others) - 1;
  return changeEach(scene, ids, (shape) => ({ ...shape, depth }));
}

/** The selected shapes made occluders (in front of the rest: they hide), or targets again when all of them are. */
export function toggledOccluders(scene: DraftScene, ids: readonly string[]): DraftScene {
  if (ids.every((id) => scene.occluders.includes(id))) return loosened(scene, ids);
  const loose = loosened(scene, ids);
  return inFront({ ...loose, occluders: [...loose.occluders, ...ids] }, ids, true);
}

/** The selected shapes taken away: a model box is crossed out (no target there), a drawn shape is gone. */
export function removed(scene: DraftScene, ids: readonly string[]): DraftScene {
  const gone = scene.shapes.filter((shape) => ids.includes(shape.id));
  const crossed = [
    ...scene.crossed,
    ...gone.flatMap((shape) => (shape.model === null ? [] : [shape.model])),
  ];
  const loose = loosened(scene, ids);
  return { ...loose, shapes: loose.shapes.filter((shape) => !ids.includes(shape.id)), crossed };
}

/** A crossed-out model box brought back as a pill. */
export function uncrossed(scene: DraftScene, crop: CropEntry, index: number): DraftScene {
  const shape = pill(`m${index}`, crop.boxes[index], index);
  return {
    ...scene,
    shapes: [...scene.shapes.filter((other) => other.id !== shape.id), shape],
    crossed: scene.crossed.filter((model) => model !== index),
  };
}

/** A tapped tiny target: a round pill of the crop's typical box size at the point. */
export function withPoint(scene: DraftScene, crop: CropEntry, x: number, y: number): DraftScene {
  const side = pointSide(crop);
  return withShape(scene, 'pill', [x, y, side, side]);
}
