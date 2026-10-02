import { ModelList } from '../api';
import { modelTable } from './model-table';

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
  const t = modelTable(LIST);
  const row = (label: string) => t.rows.find((r) => r.label.startsWith(label));

  it('has a column per current model, saying which is the default and which is in use', () => {
    expect(t.columns.map((c) => [c.name, c.isDefault, c.inUse])).toEqual([
      ['full_v3', true, false],
      ['small_v13', false, true],
      ['hand', false, false],
    ]);
    expect(t.columns[2].useLabel).toBe('hand-written');
  });

  it('marks the most flicks measured as best, and a model without the check gets no value', () => {
    const r = row('Static runs (854 kills)');
    expect(r?.cells.map((c) => [c.text, c.detail, c.best])).toEqual([
      ['844 flicks', '854 matched', false],
      ['846 flicks', '854 matched', true],
      [null, null, false],
    ]);
  });

  it('marks the lowest tracking error and the fastest speed as best', () => {
    expect(row('Tracking')?.cells.map((c) => c.best)).toEqual([true, false, false]);
    expect(row('Tracking')?.cells[0]).toMatchObject({ text: '0.031', detail: 'mean 0.010' });
    expect(row('GPU')?.cells.map((c) => [c.text, c.best])).toEqual([
      ['1.37 ms', false],
      ['0.71 ms', true],
      [null, false],
    ]);
  });

  it('marks no best in the rows of words and of sizes', () => {
    expect(row('Trained on')?.prose).toBe(true);
    expect(row('Trained on')?.cells.map((c) => c.text)).toEqual([
      'Every kind.',
      null,
      'No training.',
    ]);
    expect(row('Parameters')?.cells.some((c) => c.best)).toBe(false);
    expect(row('File size')?.cells.some((c) => c.best)).toBe(false);
  });

  it('lists the older models apart, with their size', () => {
    expect(t.older.map((c) => [c.name, c.size, c.available])).toEqual([
      ['small_v9', 'PyTorch file only', false],
    ]);
    expect(t.notes).toBe('Recordings no model trained on. Per frame.');
  });
});
