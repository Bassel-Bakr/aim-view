import { CropAnswer, CropBox, CropEntry } from '../api';

/**
 * What the user's fixes on a recording so far teach about its other crops, as the claude.ai check page learnt them
 * (python/model/crop_check/index.html, `learn` and `suggestion`): the typical correction of the model's boxes, the
 * score below which every box was crossed out, and one box per target over a target the model sees in pieces.
 */

/** A crop's boxes as a suggestion changes them: crossed out, moved or resized, and drawn. */
export interface CropFix {
  remove: number[];
  edit: Record<string, CropBox>;
  add: CropBox[];
}

/** A correction of the model's boxes: their width and height scaled, their middle moved (in their sizes). */
interface BoxCorrection {
  scaleW: number;
  scaleH: number;
  shiftX: number;
  shiftY: number;
  fixes: number;
}

/** One box per target over the model's pieces: its size, and where its middle sits from the pieces' (its sizes). */
interface WholeBox {
  width: number;
  height: number;
  offsetX: number;
  offsetY: number;
  fixes: number;
}

/** What a recording's fixes teach. */
export interface Lesson {
  correction?: BoxCorrection;
  /** The score below which every box was crossed out and above which every one was kept. */
  cut?: number;
  whole?: WholeBox;
}

/** A suggestion for a crop not yet answered, and why: a preset (a mined false box) or a lesson. */
export interface Suggestion {
  fix: CropFix;
  why: 'preset' | 'lesson' | 'whole';
  lesson: Lesson | null;
}

/** A model box and the box the user fixed it to. */
type BoxPair = [model: CropBox, fixed: CropBox];

/** The sides of the box round boxes, in crop pixels. */
type BoxSides = [left: number, top: number, right: number, bottom: number];

/** What one recording's answers hold: box pairs (the model's and the fixed one), scores crossed out and kept. */
interface Evidence {
  pairs: BoxPair[];
  removed: number[];
  kept: number[];
  wholes: WholeBox[];
}

/** A correction needs this many fixes or more, their sizes within this ratio of each other. */
const MIN_FIXES = 3;
const AGREE_RATIO = 1.35;
/** A correction smaller than this (a share of the box) is no correction. */
const NO_CHANGE = 0.05;
/** A score cut needs this many boxes crossed out and kept. */
const MIN_SCORES = 2;
/** Pieces belong to one target when their middles are this many of its boxes' sizes apart, or less. */
const SAME_TARGET = 0.75;
/** A crop's side, in pixels. */
const CROP_SIDE = 256;

const median = (values: number[]) => {
  const sorted = [...values].sort((a, b) => a - b);
  const middle = sorted.length >> 1;
  return sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
};
const tenth = (value: number) => Math.round(value * 10) / 10;

/** The box round boxes: [x0, y0, x1, y1]. */
function around(boxes: readonly CropBox[]): BoxSides {
  return [
    Math.min(...boxes.map((b) => b[0] - b[2] / 2)),
    Math.min(...boxes.map((b) => b[1] - b[3] / 2)),
    Math.max(...boxes.map((b) => b[0] + b[2] / 2)),
    Math.max(...boxes.map((b) => b[1] + b[3] / 2)),
  ];
}

/** An answer that crossed out every model box and drew one box over all their middles; null for any other. */
function wholeFix(crop: CropEntry, answer: CropAnswer): WholeBox | null {
  const drawn = answer.add.filter((box) => box.length === 4) as CropBox[];
  if (!crop.boxes.length || answer.remove.length !== crop.boxes.length || drawn.length !== 1)
    return null;
  const [cx, cy, width, height] = drawn[0];
  const inside = (b: CropBox) =>
    Math.abs(b[0] - cx) <= width / 2 && Math.abs(b[1] - cy) <= height / 2;
  if (!crop.boxes.every(inside)) return null;
  const [x0, y0, x1, y1] = around(crop.boxes);
  const [offsetX, offsetY] = [(cx - (x0 + x1) / 2) / width, (cy - (y0 + y1) / 2) / height];
  return { width, height, offsetX, offsetY, fixes: 1 };
}

/** The pairs of an answer's drawn boxes with the crossed-out model box each covers, nearest first. */
function drawnPairs(crop: CropEntry, answer: CropAnswer, replaced: Set<number>): BoxPair[] {
  const pairs: BoxPair[] = [];
  for (const drawn of answer.add.filter((box) => box.length === 4) as CropBox[]) {
    let best = -1;
    let bestDistance = Infinity;
    for (const index of answer.remove) {
      const [cx, cy] = crop.boxes[index];
      const covered =
        Math.abs(cx - drawn[0]) <= drawn[2] / 2 && Math.abs(cy - drawn[1]) <= drawn[3] / 2;
      const distance = Math.hypot(cx - drawn[0], cy - drawn[1]);
      if (!replaced.has(index) && covered && distance < bestDistance)
        [best, bestDistance] = [index, distance];
    }
    if (best >= 0) {
      replaced.add(best);
      pairs.push([crop.boxes[best], drawn]);
    }
  }
  return pairs;
}

/** What one answer adds to its recording's evidence. */
function gather(evidence: Evidence, crop: CropEntry, answer: CropAnswer): void {
  const whole = wholeFix(crop, answer);
  if (whole) evidence.wholes.push(whole);
  for (const [index, box] of Object.entries(answer.edit))
    evidence.pairs.push([crop.boxes[Number(index)], box]);
  const replaced = new Set<number>();
  evidence.pairs.push(...drawnPairs(crop, answer, replaced));
  crop.boxes.forEach((_box, index) => {
    const score = crop.scores[index];
    if (score === undefined) return;
    if (answer.remove.includes(index) && !replaced.has(index)) evidence.removed.push(score);
    else if (!answer.remove.includes(index)) evidence.kept.push(score);
  });
}

/** A recording's lesson from its evidence; null when it teaches nothing. */
function lessonOf(evidence: Evidence): Lesson | null {
  const lesson: Lesson = {};
  const widths = evidence.pairs.map(([model, fixed]) => fixed[2] / model[2]);
  const heights = evidence.pairs.map(([model, fixed]) => fixed[3] / model[3]);
  const agree = (values: number[]) => Math.max(...values) / Math.min(...values) <= AGREE_RATIO;
  if (evidence.pairs.length >= MIN_FIXES && agree(widths) && agree(heights)) {
    const [scaleW, scaleH] = [median(widths), median(heights)];
    const shiftX = median(evidence.pairs.map(([model, fixed]) => (fixed[0] - model[0]) / model[2]));
    const shiftY = median(evidence.pairs.map(([model, fixed]) => (fixed[1] - model[1]) / model[3]));
    const changes = [scaleW - 1, scaleH - 1, shiftX, shiftY].some(
      (change) => Math.abs(change) > NO_CHANGE,
    );
    if (changes)
      lesson.correction = { scaleW, scaleH, shiftX, shiftY, fixes: evidence.pairs.length };
  }
  const { removed, kept } = evidence;
  if (
    removed.length >= MIN_SCORES &&
    kept.length >= MIN_SCORES &&
    Math.max(...removed) < Math.min(...kept)
  )
    lesson.cut = (Math.max(...removed) + Math.min(...kept)) / 2;
  if (evidence.wholes.length >= MIN_FIXES) {
    const pick = (key: keyof WholeBox) => median(evidence.wholes.map((whole) => whole[key]));
    lesson.whole = {
      width: pick('width'),
      height: pick('height'),
      offsetX: pick('offsetX'),
      offsetY: pick('offsetY'),
      fixes: evidence.wholes.length,
    };
  }
  return lesson.correction || lesson.cut !== undefined || lesson.whole ? lesson : null;
}

/**
 * Each recording's lesson from the answers so far: answers of sets that do not teach, unsure ones and suggestions
 * taken as offered are left out (they would teach themselves).
 */
export function learn(
  crops: readonly CropEntry[],
  answers: Readonly<Record<string, CropAnswer>>,
): Map<string, Lesson> {
  const evidence = new Map<string, Evidence>();
  for (const crop of crops) {
    const answer = answers[crop.id];
    if (!answer || answer.verdict === 'unsure' || answer.suggested) continue;
    let found = evidence.get(crop.folder);
    if (!found) {
      found = { pairs: [], removed: [], kept: [], wholes: [] };
      evidence.set(crop.folder, found);
    }
    gather(found, crop, answer);
  }
  const lessons = new Map<string, Lesson>();
  for (const [folder, found] of evidence) {
    const lesson = lessonOf(found);
    if (lesson) lessons.set(folder, lesson);
  }
  return lessons;
}

/** One box per target over the model's pieces, as the whole-target lesson draws them. */
function wholeBoxes(crop: CropEntry, whole: WholeBox): CropBox[] {
  const groups: CropBox[][] = [];
  for (const box of crop.boxes) {
    const group = groups.find(
      (members) =>
        Math.abs(members[0][0] - box[0]) <= SAME_TARGET * whole.width &&
        Math.abs(members[0][1] - box[1]) <= SAME_TARGET * whole.height,
    );
    if (group) group.push(box);
    else groups.push([box]);
  }
  return groups.map((group) => {
    const [x0, y0, x1, y1] = around(group);
    const clamp = (value: number) => Math.min(Math.max(value, 0), CROP_SIDE);
    const cx = clamp((x0 + x1) / 2 + whole.offsetX * whole.width);
    const cy = clamp((y0 + y1) / 2 + whole.offsetY * whole.height);
    return [cx, cy, whole.width, whole.height].map(tenth) as CropBox;
  });
}

/**
 * What the page offers for a crop not yet answered: its preset (a mined false box starts crossed out), else its
 * recording's lesson applied (boxes corrected, low scores crossed out, or one box per target); null when neither.
 */
export function suggest(
  crop: CropEntry,
  lesson: Lesson | null,
  learns: boolean,
): Suggestion | null {
  if (crop.preset)
    return {
      fix: { remove: [...crop.preset.remove], edit: {}, add: [] },
      why: 'preset',
      lesson: null,
    };
  if (!lesson || !learns) return null;
  if (lesson.whole && crop.boxes.length) {
    const fix = {
      remove: crop.boxes.map((_box, index) => index),
      edit: {},
      add: wholeBoxes(crop, lesson.whole),
    };
    return { fix, why: 'whole', lesson };
  }
  const fix: CropFix = { remove: [], edit: {}, add: [] };
  crop.boxes.forEach((box, index) => {
    const score = crop.scores[index];
    const correction = lesson.correction;
    if (lesson.cut !== undefined && score !== undefined && score < lesson.cut)
      fix.remove.push(index);
    else if (correction) {
      fix.edit[String(index)] = [
        box[0] + correction.shiftX * box[2],
        box[1] + correction.shiftY * box[3],
        box[2] * correction.scaleW,
        box[3] * correction.scaleH,
      ].map(tenth) as CropBox;
    }
  });
  return fix.remove.length || Object.keys(fix.edit).length ? { fix, why: 'lesson', lesson } : null;
}
