/**
 * The kinds an excluded area can be, and KovOBS's default layout, as the review service has them
 * (service/src/areas.rs). In: areas and kind edits. Out: kind ids for the areas
 * (server-area-labels.ts), the built-in kinds (browser-labelling.ts), and the checks the specs'
 * fake review server answers with (area-labels.spec.ts).
 */

import { AreaBox, AreaKind, KindEdit } from '../../api';

/**
 * The kinds an area can be before the user adds any (python/retired/review.py: EXCLUDE_KINDS;
 * server.py: KIND_ABOUT; service/src/areas.rs: BUILT_IN).
 */
const BUILT_IN: readonly KindEdit[] = [
  {
    id: null,
    name: 'Session stats',
    about: "KovaaK's SESSION box (kills, accuracy, damage), or a game's score and accuracy boxes",
  },
  { id: null, name: 'Timer', about: "the run's time left" },
  { id: null, name: 'Clock', about: 'the time of day, a session clock, FPS' },
  { id: null, name: 'Scenario name', about: "the scenario's name" },
  { id: null, name: 'Magazine', about: 'the ammo count' },
  { id: null, name: 'Weapon', about: "the weapon's name or model" },
  { id: null, name: 'Settings', about: 'a box of settings (sensitivity, FOV, theme, sounds)' },
  { id: null, name: 'Webcam', about: 'a hand cam, face cam or avatar' },
  {
    id: null,
    name: 'Zoomed crosshair',
    about: 'a magnified view of the screen round the crosshair',
  },
  { id: null, name: 'Version', about: 'a version number' },
  { id: null, name: 'Other', about: 'anything else that is not the game' },
];

/** The kind an area has when it says none, or one that is not known. */
export const OTHER = 'other';

/**
 * A kind's id made from its name, as python/retired/server.py made it (`_slug`): lower case, each
 * run of other characters one underscore; "type" when nothing is left.
 */
export function slug(name: string): string {
  return (
    name
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '_')
      .replace(/^_+|_+$/g, '') || 'type'
  );
}

/** A new kind's id: its name's slug, with a number after it when that is taken (`_new_id`). */
export function newKindId(name: string, kinds: readonly AreaKind[]): string {
  const base = slug(name);
  const have = new Set(kinds.map((kind) => kind.id));
  let out = base;
  for (let copyNumber = 2; have.has(out); copyNumber++) out = `${base}_${copyNumber}`;
  return out;
}

/** The built-in kinds, with their ids. */
export function builtInKinds(): AreaKind[] {
  return BUILT_IN.map((kind) => ({ id: slug(kind.name), name: kind.name, about: kind.about }));
}

/**
 * A kind's id from its id or its name (areas from before kinds had ids held names; KovOBS's layout
 * still does), or from its name's slug (a built-in kind the user renamed); unknown: "other".
 */
export function kindId(value: string, kinds: readonly AreaKind[]): string {
  if (kinds.some((kind) => kind.id === value)) return value;
  const name = value.toLowerCase();
  const named = kinds.find((kind) => kind.name.toLowerCase() === name);
  if (named) return named.id;
  const id = slug(value);
  return kinds.some((kind) => kind.id === id) ? id : OTHER;
}

/** Areas with their kinds as ids. */
export function withKindIds(boxes: readonly AreaBox[], kinds: readonly AreaKind[]): AreaBox[] {
  return boxes.map(([x0, y0, x1, y1, kind]) => [x0, y0, x1, y1, kindId(kind ?? OTHER, kinds)]);
}

/**
 * The KovOBS overlay's boxes in pixels of a 1280 x 720 frame, each with its kind's id
 * (python/retired/review.py: OVERLAY and OVERLAY_KINDS; src/geometry.rs: OVERLAY).
 */
const OVERLAY: readonly AreaBox[] = [
  [0, 0, 205, 150, 'session_stats'],
  [590, 0, 690, 60, 'timer'],
  [1160, 0, 1280, 100, 'clock'],
  [0, 620, 430, 720, 'settings'],
  [570, 630, 715, 720, 'weapon'],
  [400, 675, 880, 720, 'scenario_name'],
  [960, 535, 1280, 720, 'webcam'],
  [0, 700, 60, 720, 'version'],
];

/**
 * KovOBS's layout: the areas excluded by default, as shares of the frame (OVERLAY_SHARES: each
 * bound divided once).
 */
export function kovobsLayout(): AreaBox[] {
  return OVERLAY.map(([x0, y0, x1, y1, kind]) => [x0 / 1280, y0 / 720, x1 / 1280, y1 / 720, kind]);
}

/**
 * Whether a list of areas is one the review can use: each inside the frame, with its start before
 * its end.
 */
export function validAreas(boxes: unknown): boxes is AreaBox[] {
  return (
    Array.isArray(boxes) &&
    boxes.every(
      (b: unknown) =>
        Array.isArray(b) &&
        (b.length === 4 || (b.length === 5 && typeof b[4] === 'string')) &&
        b
          .slice(0, 4)
          .every((value: unknown) => typeof value === 'number' && Number.isFinite(value)) &&
        0 <= b[0] &&
        b[0] < b[2] &&
        b[2] <= 1 &&
        0 <= b[1] &&
        b[1] < b[3] &&
        b[3] <= 1,
    )
  );
}

/**
 * The kinds after an edit: a new kind added (id null), or a kind's new name and description,
 * checked as python/retired/server.py checked them (`save_kind`): the name up to 40 characters,
 * the description up to 200, no two kinds of one name. Throws when not valid.
 */
export function editKinds(kinds: readonly AreaKind[], edit: KindEdit): AreaKind[] {
  const name = edit.name.trim().slice(0, 40);
  const about = edit.about.trim().slice(0, 200);
  if (!name) throw new Error('a type needs a name');
  if (kinds.some((kind) => kind.name.toLowerCase() === name.toLowerCase() && kind.id !== edit.id)) {
    throw new Error(`there is a type called ${name} already`);
  }
  if (edit.id === null) return [...kinds, { id: newKindId(name, kinds), name, about }];
  if (!kinds.some((kind) => kind.id === edit.id)) throw new Error(`no type with the id ${edit.id}`);
  return kinds.map((kind) => (kind.id === edit.id ? { ...kind, name, about } : kind));
}
