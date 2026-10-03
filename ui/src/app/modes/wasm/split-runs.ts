import { RunMarks, TimeWindow } from '../../api';

/**
 * A run of a recording's frames, reviewed in a worker of its own: it starts at a key frame's time (`from`; the
 * recording's first run at 0) and, but for the last run, stops before the next run's (`to`, reading that run's first
 * frame for the camera's turn into it); the last run stops after its frames (`to` null). `first` is its first frame's
 * index in the recording, `frames` how many it has.
 */
export interface VideoRun {
  from: number;
  to: number | null;
  first: number;
  frames: number;
}

/** The frames to review: from `first` up to `end` (not included), as indexes in the recording. */
export interface FrameRange {
  first: number;
  end: number;
}

/** Seconds tracked either side of the user's run window, so its first and last kills are whole. */
const MARGIN = 1;

/**
 * The part of the video to track for the user's run window (python/review.py's run_window, in seconds, with a
 * margin): start and end, or one of them and the length (else the scenario's limit); null for the whole video.
 */
export function trackedWindow(marks: RunMarks | null, limit: number | null): TimeWindow | null {
  if (!marks) return null;
  const { start: a, end: b } = marks;
  const length = marks.length || limit || null;
  let span: TimeWindow;
  if (a != null && b != null && b > a) span = { start: a, end: b };
  else if (a != null) span = { start: a, end: length ? a + length : Infinity };
  else if (b != null && length) span = { start: Math.max(0, b - length), end: b };
  else return null;
  return { start: Math.max(0, span.start - MARGIN), end: span.end + MARGIN };
}

/** Whether tracks made over `tracked` (null: the whole video) hold all of `wanted` (null: the whole video). */
export function covers(tracked: TimeWindow | null, wanted: TimeWindow | null): boolean {
  if (!tracked) return true;
  return !!wanted && tracked.start <= wanted.start && tracked.end >= wanted.end;
}

/** The frames a time window holds: from the first at or after its start to the last at or before its end. */
export function windowFrames(times: readonly number[], window: TimeWindow | null): FrameRange {
  const all = { first: 0, end: times.length };
  if (!window) return all;
  const first = times.findIndex((t) => t >= window.start);
  const after = times.findIndex((t) => t > window.end);
  const range = { first: Math.max(0, first), end: after < 0 ? times.length : after };
  return first < 0 || range.end <= range.first ? all : range;
}

/**
 * A recording's frames split into up to `parts` runs, each starting at a key frame, so the runs can be decoded at
 * once (one software decoder is the review's limit on the GPU). `times`: every frame's time from 0 on, in order;
 * `keys`: the key frames' times. Only the frames in `range` are reviewed, from the key frame at or before its first
 * (decoding starts at a key frame). Each cut is at the key frame nearest its share of the frames; a cut that would leave
 * a run of fewer than `least` frames is not made. Every worker works the runs out the same way from the same file.
 */
export function splitRuns(
  times: readonly number[],
  keys: readonly number[],
  parts: number,
  least: number,
  range: FrameRange = { first: 0, end: times.length },
): VideoRun[] {
  let begin = 0;
  for (const k of keys) {
    const i = times.indexOf(k);
    if (i > begin && i <= range.first) begin = i;
  }
  const n = range.end - begin;
  const starts = [begin];
  for (let i = 1; i < parts; i++) {
    const want = times[begin + Math.floor((n * i) / parts)];
    let best = -1;
    for (const k of keys) {
      if (k > times[begin] && (best < 0 || Math.abs(k - want) < Math.abs(best - want))) best = k;
    }
    const at = times.indexOf(best);
    if (at - starts[starts.length - 1] >= least && range.end - at >= least) starts.push(at);
  }
  return starts.map((first, i) => {
    const next = i + 1 < starts.length ? starts[i + 1] : null;
    return {
      from: first ? times[first] : 0,
      to: next === null ? null : times[next],
      first,
      frames: (next ?? range.end) - first,
    };
  });
}
