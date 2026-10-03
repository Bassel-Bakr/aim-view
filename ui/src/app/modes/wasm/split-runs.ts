/**
 * A run of a recording's frames, reviewed in a worker of its own: it starts at a key frame's time (`from`; the first
 * run at 0) and stops before the next run's (`to`; null for the last run). `first` is its first frame's index in the
 * recording, `frames` how many it has.
 */
export interface VideoRun {
  from: number;
  to: number | null;
  first: number;
  frames: number;
}

/**
 * A recording's frames split into up to `parts` runs, each starting at a key frame, so the runs can be decoded at
 * once (one software decoder is the review's limit on the GPU). `times`: every frame's time from 0 on, in order;
 * `keys`: the key frames' times. Each cut is at the key frame nearest its share of the frames; a cut that would leave
 * a run of fewer than `least` frames is not made. Every worker works the runs out the same way from the same file.
 */
export function splitRuns(
  times: readonly number[],
  keys: readonly number[],
  parts: number,
  least: number,
): VideoRun[] {
  const n = times.length;
  const starts = [0];
  for (let i = 1; i < parts; i++) {
    const want = times[Math.floor((n * i) / parts)];
    let best = -1;
    for (const k of keys) {
      if (k > times[0] && (best < 0 || Math.abs(k - want) < Math.abs(best - want))) best = k;
    }
    const at = times.indexOf(best);
    if (at - starts[starts.length - 1] >= least && n - at >= least) starts.push(at);
  }
  return starts.map((first, i) => {
    const next = i + 1 < starts.length ? starts[i + 1] : null;
    return {
      from: i ? times[first] : 0,
      to: next === null ? null : times[next],
      first,
      frames: (next ?? n) - first,
    };
  });
}
