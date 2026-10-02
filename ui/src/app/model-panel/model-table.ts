import { Model, ModelList, ModelSpeed } from '../api';
import { formatNumber } from '../format';
import { modelName } from '../services/models';

/** A cell's words: its value, and a detail after it. */
export interface CellText {
  text: string;
  detail: string | null;
}

/** A cell of the table. text null: no value. best: the best in its row (said in words too). */
export interface ModelCell {
  text: string | null;
  detail: string | null;
  best: boolean;
}

/** A row: one measure, or one kind of words (prose), for every model. about: what it measures. */
export interface ModelRow {
  label: string;
  about: string | null;
  prose: boolean;
  cells: ModelCell[];
}

/** A model the panel shows, with what its button says. */
export interface ModelColumn {
  name: string;
  label: string;
  useLabel: string;
  size: string;
  isDefault: boolean;
  inUse: boolean;
  available: boolean;
}

/** The models side by side (one column each), how they were measured, and the older models in a list under them. */
export interface ModelTable {
  columns: ModelColumn[];
  rows: ModelRow[];
  notes: string;
  older: ModelColumn[];
}

/** Which way a row's best value lies, or null for a row with no best. */
export type Better = 'low' | 'high' | null;

const EMPTY: ModelCell = { text: null, detail: null, best: false };

function column(m: Model, chosen: string): ModelColumn {
  return {
    name: m.name,
    label: m.label,
    useLabel: modelName(m.name),
    size: m.kb != null ? `${m.kb} KB` : 'PyTorch file only',
    isDefault: m.default === true,
    inUse: m.name === chosen,
    available: m.available,
  };
}

function proseRow(label: string, values: (string | undefined)[]): ModelRow {
  return {
    label,
    about: null,
    prose: true,
    cells: values.map((v) => (v ? { text: v, detail: null, best: false } : EMPTY)),
  };
}

/** A row of numbers; with two or more values, the lowest or highest is marked best. */
export function numberRow(
  label: string,
  about: string | null,
  values: (number | null)[],
  show: (v: number, i: number) => CellText,
  better: Better,
): ModelRow {
  const have = values.filter((v): v is number => v !== null);
  const best =
    better && have.length > 1 ? (better === 'low' ? Math.min(...have) : Math.max(...have)) : null;
  return {
    label,
    about,
    prose: false,
    cells: values.map((v, i) => (v === null ? EMPTY : { ...show(v, i), best: v === best })),
  };
}

const plain = (text: string): CellText => ({ text, detail: null });

/**
 * The model panel's table: what each model was trained on, is best and weak at, its result on every check, its speed
 * on each runtime and its size. Older models (each replaced by a newer one) are listed apart.
 */
export function modelTable(list: ModelList): ModelTable {
  const main = list.models.filter((m) => !m.older);
  const checks = list.checks.map((c) => {
    const got = main.map((m) => m.checks?.[c.key] ?? null);
    const values = got.map((g) => (g ? g[1] : null));
    if (c.key === 'tracking') {
      return numberRow(
        c.name,
        c.what,
        values,
        (v, i) => ({ text: v.toFixed(3), detail: `mean ${got[i]?.[0].toFixed(3)}` }),
        'low',
      );
    }
    return numberRow(
      c.of ? `${c.name} (${formatNumber(c.of)} kills)` : c.name,
      c.what,
      values,
      (v, i) => ({
        text: `${formatNumber(v)} flicks`,
        detail: `${formatNumber(got[i]?.[0] ?? 0)} matched`,
      }),
      'high',
    );
  });
  const speed = (key: keyof ModelSpeed) => main.map((m) => m.speed_ms?.[key] ?? null);
  const ms = (v: number) => plain(`${v} ms`);
  return {
    columns: main.map((m) => column(m, list.chosen)),
    rows: [
      proseRow(
        'Trained on',
        main.map((m) => m.trained),
      ),
      proseRow(
        'Best at',
        main.map((m) => m.best),
      ),
      proseRow(
        'Weak at',
        main.map((m) => m.weak),
      ),
      ...checks,
      numberRow('GPU, per frame', 'PyTorch, batches of 16', speed('gpu'), ms, 'low'),
      numberRow('CPU, per frame', 'ONNX Runtime, fp32, 4 threads', speed('cpu'), ms, 'low'),
      numberRow('Browser, per frame', 'onnxruntime-web, WASM', speed('browser'), ms, 'low'),
      numberRow(
        'Parameters',
        null,
        main.map((m) => m.params ?? null),
        (v) => plain(formatNumber(v)),
        null,
      ),
      numberRow(
        'File size',
        'The fp32 ONNX file',
        main.map((m) => m.kb ?? null),
        (v) => plain(`${v} KB`),
        null,
      ),
    ],
    notes: `${list.checked_on} ${list.speed}`,
    older: list.models.filter((m) => m.older).map((m) => column(m, list.chosen)),
  };
}
