/**
 * A tracking run's timeline as shapes, for its SVG: how far outside the bot's edge the crosshair was, per pixel column
 * the furthest and the average; under it a strip of on target, off target and switching, the bots' deaths, and the
 * seconds.
 *
 * In: the run moment by moment (track.ts `timeline`: each frame's state and distance off the bot, the bots' deaths),
 * the chart's width on screen, and its sizes from the tokens (tokens.ts, from themes/timeline.scss; the same in both
 * themes). Out: the shapes timeline.html draws; their colors are the stylesheet's (timeline.scss).
 */

import { TOKENS } from '../../tokens/tokens';
import { Timeline, TrackState } from '../track';

/** Space above the chart, in CSS pixels. */
const TOP = 6;
/** Space between the chart and the strip, in CSS pixels. */
const STRIP_GAP = 8;
/** Space under the strip for the seconds, in CSS pixels. */
const AXIS = 30;
/** The grid's lines: at no distance, half the scale and its top. */
const GRID_SHARES = [0, 0.5, 1];
/** Lines sit on a pixel's center, so they stay sharp. */
const HALF_PIXEL = 0.5;
/** A scale label's height, in CSS pixels. */
const LABEL_HEIGHT = 14;
/** A seconds label's gap from the bottom, in CSS pixels. */
const SECONDS_PAD = 3;
/** A death's mark is this wide, in CSS pixels. */
const DEATH_WIDTH = 1.5;
/** A death's mark rises this far above the strip, in CSS pixels. */
const DEATH_RISE = 4;
/** Seconds between the seconds' labels. */
const STEP_SECONDS = 10;
/** Seconds between the seconds' labels on a run longer than `LONG_RUN_SECONDS`. */
const LONG_STEP_SECONDS = 20;
/** A run longer than this, in seconds, gets the longer step between labels. */
const LONG_RUN_SECONDS = 90;
/** A seconds label this near the right edge, in CSS pixels, is right-aligned so it stays whole. */
const RIGHT_EDGE = 20;
/** The chart's height, in CSS pixels (the box's, themes/timeline.scss). */
const HEIGHT = parseFloat(TOKENS.dark['timeline-height']);
/** The strip's height, in CSS pixels. */
const STRIP = Number(TOKENS.dark['timeline-strip-height']);

/** A grid line across the chart. */
export interface GridLine {
  /** Its height, in CSS pixels from the top. */
  y: number;
  /** Whether it is dashed (half the scale and its top) or solid (no distance). */
  dashed: boolean;
}

/** A label on the scale, at the chart's left edge. */
export interface ScaleLabel {
  /** Its words. */
  text: string;
  /** Its top, in CSS pixels. */
  top: number;
}

/** A seconds label along the bottom. */
export interface SecondsLabel {
  /** Its words. */
  text: string;
  /** Where it is anchored, in CSS pixels from the left. */
  x: number;
  /** How it sits on x: its start, middle or end. */
  anchor: 'start' | 'middle' | 'end';
}

/** The strip's shapes, one path per state. */
export interface StripPaths {
  /** Where no bot was found. */
  noBot: string;
  /** On the bot. */
  on: string;
  /** Off the bot. */
  off: string;
  /** Switching after a bot's death. */
  switching: string;
}

/** The timeline's shapes, in CSS pixels. */
export interface TimelineChart {
  /** The chart's width. */
  width: number;
  /** The chart's height. */
  height: number;
  /** The grid's lines. */
  grid: GridLine[];
  /** The furthest distance per pixel column, as a filled area. */
  furthest: string;
  /** The average distance per pixel column, as a filled area. */
  mean: string;
  /** The strip: per pixel column, the state most of its frames had. */
  strip: StripPaths;
  /** The bots' deaths, as marks across the strip. */
  deaths: string;
  /** The scale's top and bottom. */
  scale: ScaleLabel[];
  /** The seconds along the bottom. */
  seconds: SecondsLabel[];
  /** The seconds labels' baseline, from the top. */
  secondsY: number;
}

/**
 * Calls back with the frames each pixel column covers: from firstFrame up to endFrame (at least one frame), counted
 * from the run's start.
 */
function columns(
  run: Timeline,
  widthPx: number,
  each: (firstFrame: number, endFrame: number, column: number) => void,
): void {
  for (let column = 0; column < widthPx; column++) {
    const firstFrame = Math.floor((column * run.frameCount) / widthPx);
    const endFrame = Math.max(
      firstFrame + 1,
      Math.floor(((column + 1) * run.frameCount) / widthPx),
    );
    each(firstFrame, endFrame, column);
  }
}

/** Per pixel column, the average and the furthest distance outside the bot's edge, in degrees. */
interface ColumnDistances {
  /** Each column's average distance over the frames with one; 0 when none has. */
  mean: number[];
  /** Each column's furthest distance; 0 when no frame has one. */
  furthest: number[];
}

/** The distances outside the bot's edge per pixel column, frames with none (NaN) left out. */
function columnDistances(run: Timeline, widthPx: number): ColumnDistances {
  const mean: number[] = [];
  const furthest: number[] = [];
  columns(run, widthPx, (firstFrame, endFrame) => {
    let sum = 0;
    let count = 0;
    let most = 0;
    for (let frame = firstFrame; frame < endFrame; frame++) {
      const distanceDeg = run.outsideDeg[frame];
      if (Number.isNaN(distanceDeg)) continue;
      sum += distanceDeg;
      count++;
      most = Math.max(most, distanceDeg);
    }
    mean.push(count ? sum / count : 0);
    furthest.push(most);
  });
  return { mean, furthest };
}

/** A filled area of values per pixel column, stepping at each column, down to `floorY`; y from `yOf`. */
function areaPath(
  values: number[],
  widthPx: number,
  floorY: number,
  yOf: (value: number) => number,
): string {
  const steps = values.map((value, column) => `L${column} ${yOf(value)}H${column + 1}`).join('');
  return `M0 ${floorY}${steps}L${widthPx} ${floorY}Z`;
}

/** The strip's paths: per pixel column the state most of its frames had, runs of one state as one rectangle. */
function stripPaths(run: Timeline, widthPx: number, stripY: number): StripPaths {
  const states: TrackState[] = [];
  columns(run, widthPx, (firstFrame, endFrame) => {
    const framesPerState = [0, 0, 0, 0];
    for (let frame = firstFrame; frame < endFrame; frame++) framesPerState[run.state[frame]]++;
    states.push(framesPerState.indexOf(Math.max(...framesPerState)) as TrackState);
  });
  const paths: Record<TrackState, string> = {
    [TrackState.NoBot]: '',
    [TrackState.On]: '',
    [TrackState.Off]: '',
    [TrackState.Switching]: '',
  };
  for (let start = 0; start < states.length;) {
    let end = start + 1;
    while (end < states.length && states[end] === states[start]) end++;
    paths[states[start]] += `M${start} ${stripY}H${end}V${stripY + STRIP}H${start}Z`;
    start = end;
  }
  return {
    noBot: paths[TrackState.NoBot],
    on: paths[TrackState.On],
    off: paths[TrackState.Off],
    switching: paths[TrackState.Switching],
  };
}

/** The seconds along the bottom, a label every ten (or twenty, on a long run) seconds. */
function secondsLabels(run: Timeline, widthPx: number): SecondsLabel[] {
  const seconds = run.frameCount / run.fps;
  const step = seconds > LONG_RUN_SECONDS ? LONG_STEP_SECONDS : STEP_SECONDS;
  const labels: SecondsLabel[] = [];
  for (let second = 0; second <= seconds; second += step) {
    const x = (second / seconds) * widthPx;
    const anchor = second === 0 ? 'start' : x > widthPx - RIGHT_EDGE ? 'end' : 'middle';
    labels.push({ text: `${second} s`, x: Math.min(widthPx - 1, x), anchor });
  }
  return labels;
}

/** The timeline's shapes for a chart `widthPx` CSS pixels wide. */
export function timelineChart(run: Timeline, widthPx: number): TimelineChart {
  const chartHeight = HEIGHT - TOP - STRIP_GAP - STRIP - AXIS;
  const stripY = TOP + chartHeight + STRIP_GAP;
  const yOf = (distanceDeg: number) =>
    TOP + chartHeight * (1 - Math.min(distanceDeg, run.capDeg) / run.capDeg);
  const distances = columnDistances(run, widthPx);
  const deaths = run.deaths
    .map((death) => {
      const x = Math.floor((death / run.frameCount) * widthPx);
      return `M${x} ${stripY - DEATH_RISE}h${DEATH_WIDTH}v${STRIP + DEATH_RISE}h${-DEATH_WIDTH}Z`;
    })
    .join('');
  return {
    width: widthPx,
    height: HEIGHT,
    grid: GRID_SHARES.map((share) => ({
      y: Math.round(yOf(share * run.capDeg)) + HALF_PIXEL,
      dashed: share > 0,
    })),
    furthest: areaPath(distances.furthest, widthPx, yOf(0), yOf),
    mean: areaPath(distances.mean, widthPx, yOf(0), yOf),
    strip: stripPaths(run, widthPx, stripY),
    deaths,
    scale: [
      { text: `${run.capDeg.toFixed(1)}° off`, top: TOP },
      { text: '0°: on the bot', top: TOP + chartHeight - LABEL_HEIGHT },
    ],
    seconds: secondsLabels(run, widthPx),
    secondsY: HEIGHT - SECONDS_PAD,
  };
}
