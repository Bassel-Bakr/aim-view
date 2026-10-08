/**
 * The model panel's comparison table, worked out from the model list: one column per model, one
 * row per measure, the best in each row marked. In: the ModelList (/api/models, from models.json).
 * Out: the model panel's choice table (model-choice/).
 */

import { Check, Model, ModelList, ModelSpeed } from '../api';
import { formatNumber } from '../format';
import { modelName } from '../services/models';

/** A cell's words: its value, and a detail after it. */
export interface CellText {
  /** The value, in words. */
  text: string;
  /** A detail after it; null for none. */
  detail: string | null;
}

/** A cell of the table. text null: no value. best: the best in its row (said in words too). */
export interface ModelCell {
  /** The value, in words; null when the model has none. */
  text: string | null;
  /** A detail after it; null for none. */
  detail: string | null;
  /** Whether it is the best in its row. */
  best: boolean;
}

/** A row: one measure, or one kind of words (prose), for every model. about: what it measures. */
export interface ModelRow {
  /** The row's name. */
  label: string;
  /** What it measures; null when its name says it. */
  about: string | null;
  /** Whether its cells are sentences, not numbers. */
  prose: boolean;
  /** One cell per model, in the columns' order. */
  cells: ModelCell[];
}

/** A model the panel shows, with what its button says. */
export interface ModelColumn {
  /** The model's id. */
  name: string;
  /** Its name in models.json. */
  label: string;
  /** Its name on its Use button. */
  useLabel: string;
  /** Its file's size in words ("587.1 KB"). */
  size: string;
  /** Whether it is models.json's default. */
  isDefault: boolean;
  /** Whether new reviews use it. */
  inUse: boolean;
  /** Whether it can run here. */
  available: boolean;
}

/** The models side by side (one column each), how they were measured, and the older models in a list under them. */
export interface ModelTable {
  /** The models side by side, the most recommended first (byRecommendation). */
  columns: ModelColumn[];
  /** The measures, one row each. */
  rows: ModelRow[];
  /** How the checks and speeds were measured, in words. */
  notes: string;
  /** The older models, listed under the table. */
  older: ModelColumn[];
  /** Why a model that is not available cannot run here. */
  unavailable: string;
}

/** Which way a row's best value lies, or null for a row with no best. */
export type Better = 'low' | 'high' | null;

/** A cell with no value. */
const EMPTY: ModelCell = { text: null, detail: null, best: false };

/** A model's column, `chosen` being the model new reviews use. */
function column(model: Model, chosen: string): ModelColumn {
  return {
    name: model.name,
    label: model.label,
    useLabel: modelName(model.name),
    size: model.kb != null ? `${model.kb} KB` : 'PyTorch file only',
    isDefault: model.default === true,
    inUse: model.name === chosen,
    available: model.available,
  };
}

/** A row of sentences, one per model (undefined: an empty cell). */
function proseRow(label: string, values: (string | undefined)[]): ModelRow {
  return {
    label,
    about: null,
    prose: true,
    cells: values.map((value) => (value ? { text: value, detail: null, best: false } : EMPTY)),
  };
}

/** A row of numbers; with two or more values, the lowest or highest is marked best. */
export function numberRow(
  label: string,
  about: string | null,
  values: (number | null)[],
  show: (value: number, i: number) => CellText,
  better: Better,
): ModelRow {
  const have = values.filter((value): value is number => value !== null);
  const best =
    better && have.length > 1 ? (better === 'low' ? Math.min(...have) : Math.max(...have)) : null;
  return {
    label,
    about,
    prose: false,
    cells: values.map((value, i) =>
      value === null ? EMPTY : { ...show(value, i), best: value === best },
    ),
  };
}

/** A cell's words with no detail. */
const plain = (text: string): CellText => ({ text, detail: null });

/**
 * A check's row: each model's result on it. Tracking's is an error (lowest best, its mean beside it); the others count
 * flicks (highest best, the kills matched beside them).
 */
function checkRow(check: Check, models: Model[]): ModelRow {
  const results = models.map((model) => model.checks?.[check.key] ?? null);
  const values = results.map((result) => (result ? result[1] : null));
  if (check.key === 'tracking') {
    return numberRow(
      check.name,
      check.what,
      values,
      (value, i) => ({ text: value.toFixed(3), detail: `mean ${results[i]?.[0].toFixed(3)}` }),
      'low',
    );
  }
  return numberRow(
    check.of ? `${check.name} (${formatNumber(check.of)} kills)` : check.name,
    check.what,
    values,
    (value, i) => ({
      text: `${formatNumber(value)} flicks`,
      detail: `${formatNumber(results[i]?.[0] ?? 0)} matched`,
    }),
    'high',
  );
}

/** The date at the start of a model's acceptance (YYYY-MM-DD), or '' when the gate never accepted it. */
function acceptedOn(model: Model): string {
  return /^\d{4}-\d{2}-\d{2}/.exec(model.accepted ?? '')?.[0] ?? '';
}

/**
 * The models, the most recommended first: the default; then those that can run here before those that cannot; then
 * the ones the acceptance gate passed, the most recently accepted first (on one day, the one models.json lists later,
 * as it lists models in the order they came); then the rest, in models.json's order.
 */
export function byRecommendation(models: Model[]): Model[] {
  const position = new Map(models.map((model, index) => [model, index]));
  return [...models].sort(
    (a, b) =>
      Number(b.default === true) - Number(a.default === true) ||
      Number(b.available) - Number(a.available) ||
      acceptedOn(b).localeCompare(acceptedOn(a)) ||
      (acceptedOn(a) ? 1 : -1) * ((position.get(b) ?? 0) - (position.get(a) ?? 0)),
  );
}

/**
 * The model panel's table: what each model was trained on, is best and weak at, its result on every check, its speed
 * on each runtime and its size. Older models (each replaced by a newer one) are listed apart.
 */
export function modelTable(list: ModelList): ModelTable {
  const main = byRecommendation(list.models.filter((model) => !model.older));
  const speed = (key: keyof ModelSpeed) => main.map((model) => model.speed_ms?.[key] ?? null);
  const ms = (value: number) => plain(`${value} ms`);
  return {
    columns: main.map((model) => column(model, list.chosen)),
    rows: [
      proseRow(
        'Trained on',
        main.map((model) => model.trained),
      ),
      proseRow(
        'Best at',
        main.map((model) => model.best),
      ),
      proseRow(
        'Weak at',
        main.map((model) => model.weak),
      ),
      ...list.checks.map((check) => checkRow(check, main)),
      numberRow('GPU, per frame', 'PyTorch, batches of 16', speed('gpu'), ms, 'low'),
      numberRow('CPU, per frame', 'ONNX Runtime, fp32, 4 threads', speed('cpu'), ms, 'low'),
      numberRow('Browser, per frame', 'onnxruntime-web, WASM', speed('browser'), ms, 'low'),
      numberRow(
        'Parameters',
        null,
        main.map((model) => model.params ?? null),
        (value) => plain(formatNumber(value)),
        null,
      ),
      numberRow(
        'File size',
        'The fp32 ONNX file',
        main.map((model) => model.kb ?? null),
        (value) => plain(`${value} KB`),
        null,
      ),
    ],
    notes: `${list.checked_on} ${list.speed}`,
    unavailable: list.unavailable ?? 'Needs the GPU',
    older: list.models.filter((model) => model.older).map((model) => column(model, list.chosen)),
  };
}
