import { rowLine } from './cutoff-labels';

describe('cut-off labels', () => {
  it('writes a row as hand_crops.py writes it in checked.jsonl', () => {
    const line = rowLine({
      file: 'train/a_000154.npz',
      boxes: [[168.2, 169.36, 5.67, 128]],
      verdict: 'correct',
      model: [],
      source: 'cutoff',
      video: 'Café.mp4',
      offset: 0.3,
      cut: 0.565,
    });
    expect(line).toBe(
      '{"file": "train/a_000154.npz", "boxes": [[168.2, 169.36, 5.67, 128.0]], "verdict": "correct", ' +
        '"model": [], "source": "cutoff", "video": "Caf\\u00e9.mp4", "offset": 0.3, "cut": 0.565}',
    );
  });
});
