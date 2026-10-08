import { Model, ModelList } from '../api';
import { byRecommendation, modelTable } from './model-table';

const LIST: ModelList = {
  chosen: 'small_v13',
  device: 'cuda',
  speed: 'Per frame.',
  checked_on: 'Recordings no model trained on.',
  checks: [
    { key: 'static', name: 'Static runs', of: 854, what: 'kills matched, flicks measured' },
    { key: 'tracking', name: 'Tracking', what: 'nearer 0 is better' },
  ],
  models: [
    {
      name: 'full_v3',
      label: 'full_v3',
      available: true,
      default: true,
      kb: 324.4,
      params: 80765,
      trained: 'Every kind.',
      speed_ms: { gpu: 1.37, cpu: 6, browser: 24.1 },
      checks: { static: [854, 844], tracking: [0.01, 0.031] },
    },
    {
      name: 'small_v13',
      label: 'small_v13',
      available: true,
      kb: 134.5,
      speed_ms: { gpu: 0.71, cpu: 4, browser: 14.2 },
      checks: { static: [854, 846], tracking: [0.02, 0.045] },
    },
    { name: 'hand', label: 'Hand-written', available: true, trained: 'No training.' },
    { name: 'small_v9', label: 'small_v9', available: false, older: true, kb: null },
  ],
};

describe('modelTable', () => {
  const table = modelTable(LIST);
  const row = (label: string) => table.rows.find((tableRow) => tableRow.label.startsWith(label));

  it('has a column per current model, saying which is the default and which is in use', () => {
    expect(table.columns.map((column) => [column.name, column.isDefault, column.inUse])).toEqual([
      ['full_v3', true, false],
      ['small_v13', false, true],
      ['hand', false, false],
    ]);
    expect(table.columns[2].useLabel).toBe('hand-written');
  });

  it('marks the most flicks measured as best, and a model without the check gets no value', () => {
    const staticRuns = row('Static runs (854 kills)');
    expect(staticRuns?.cells.map((cell) => [cell.text, cell.detail, cell.best])).toEqual([
      ['844 flicks', '854 matched', false],
      ['846 flicks', '854 matched', true],
      [null, null, false],
    ]);
  });

  it('marks the lowest tracking error and the fastest speed as best', () => {
    expect(row('Tracking')?.cells.map((cell) => cell.best)).toEqual([true, false, false]);
    expect(row('Tracking')?.cells[0]).toMatchObject({ text: '0.031', detail: 'mean 0.010' });
    expect(row('GPU')?.cells.map((cell) => [cell.text, cell.best])).toEqual([
      ['1.37 ms', false],
      ['0.71 ms', true],
      [null, false],
    ]);
  });

  it('marks no best in the rows of words and of sizes', () => {
    expect(row('Trained on')?.prose).toBe(true);
    expect(row('Trained on')?.cells.map((cell) => cell.text)).toEqual([
      'Every kind.',
      null,
      'No training.',
    ]);
    expect(row('Parameters')?.cells.some((cell) => cell.best)).toBe(false);
    expect(row('File size')?.cells.some((cell) => cell.best)).toBe(false);
  });

  it('lists the older models apart, with their size', () => {
    expect(table.older.map((column) => [column.name, column.size, column.available])).toEqual([
      ['small_v9', 'PyTorch file only', false],
    ]);
    expect(table.notes).toBe('Recordings no model trained on. Per frame.');
  });
});

describe('byRecommendation', () => {
  /** A model by its name, acceptance date and whether it can run here. */
  const model = (name: string, accepted?: string, available = true, isDefault = false): Model => ({
    name,
    label: name,
    available,
    default: isDefault,
    ...(accepted ? { accepted: `${accepted}: python/model/reports/accept_${name}.json` } : {}),
  });

  it('puts the default first, then what can run, the newest accepted first, then the rest in their order', () => {
    // models.json's order: the order the models came in
    const models = [
      model('full_v3'),
      model('full_v6', '2026-10-05'),
      model('full_v8_s3', '2026-10-05'),
      model('large_v11', '2026-10-06'),
      model('large_v13e4', '2026-10-06', true, true),
      model('hand'),
      model('gpu_only', '2026-10-07', false),
    ];
    expect(byRecommendation(models).map((item) => item.name)).toEqual([
      'large_v13e4',
      'large_v11',
      'full_v8_s3',
      'full_v6',
      'full_v3',
      'hand',
      'gpu_only',
    ]);
  });
});
