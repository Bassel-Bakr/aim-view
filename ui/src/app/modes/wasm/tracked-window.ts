import { RunMarks, TimeWindow } from '../../api';

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
