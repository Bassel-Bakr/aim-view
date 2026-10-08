/**
 * How the page writes numbers, times, sizes, shares, degrees and directions ("425 ms", "79%",
 * "0.48°", "↗"); a missing value is a dash. In: the API's values (seconds, shares, degrees,
 * bytes). Out: every feature's text.
 */

import { Direction, Kind } from './api';

/** The months' short names, January first. */
const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

/** Each scenario kind's name on the page. */
export const KIND_LABELS: Record<Kind, string> = {
  static: 'Static',
  dynamic: 'Dynamic',
  tracking: 'Tracking',
  switching: 'Switching',
};

/** 13278 as "13,278", 889.26 as "889.26". */
export function formatNumber(value: number): string {
  return value.toLocaleString('en-US', { maximumFractionDigits: 2 });
}

/**
 * A recording's time stamp ("2026.08.11-13.55.27") as "Aug 11, 13:55", with the year when it is not this one. Some
 * KovOBS recordings from June 2026 carry the year 0026.
 */
export function formatStamp(stamp: string, thisYear = new Date().getFullYear()): string {
  const match = /^(\d{4})\.(\d\d)\.(\d\d)-(\d\d)\.(\d\d)/.exec(stamp);
  if (!match) return stamp;
  const year = match[1].startsWith('00') ? `20${match[1].slice(2)}` : match[1];
  const day = `${MONTHS[Number(match[2]) - 1]} ${Number(match[3])}`;
  return `${day}${Number(year) === thisYear ? '' : ` ${year}`}, ${match[4]}:${match[5]}`;
}

/** Bytes as "68 MB". */
export function formatSize(bytes: number): string {
  return `${Math.round(bytes / BYTES_PER_MB)} MB`;
}

/** Bytes in the unit that suits them: "640 KB", "68 MB", "2.4 GB" (decimal, as file sizes are given). */
export function formatBytes(bytes: number): string {
  if (bytes < BYTES_PER_MB) return `${Math.round(bytes / BYTES_PER_KB)} KB`;
  if (bytes < BYTES_PER_GB) return formatSize(bytes);
  return `${(bytes / BYTES_PER_GB).toFixed(1)} GB`;
}

/** What a missing value shows as: a dash. */
const NONE = '–';
/** Bytes in a kilobyte (decimal, as file sizes are given). */
const BYTES_PER_KB = 1e3;
/** Bytes in a megabyte (decimal, as file sizes are given). */
const BYTES_PER_MB = 1e6;
/** Bytes in a gigabyte. */
const BYTES_PER_GB = 1e9;
/** Milliseconds in a second. */
const MS_PER_SECOND = 1000;
/** A share of 1 in percent. */
const PERCENT = 100;
/** Seconds in a minute. */
const SECONDS_PER_MINUTE = 60;
/** Seconds in an hour. */
const SECONDS_PER_HOUR = 3600;
/** Seconds in a day. */
const SECONDS_PER_DAY = 86400;

/** Seconds as "425 ms". */
export function formatMs(seconds: number | null | undefined): string {
  return seconds == null ? NONE : `${Math.round(MS_PER_SECOND * seconds)} ms`;
}

/** Seconds as milliseconds under a second ("167 ms"), else seconds ("1.25 s"). */
export function formatSeconds(seconds: number | null | undefined): string {
  if (seconds == null) return NONE;
  return seconds < 1 ? formatMs(seconds) : `${seconds.toFixed(2)} s`;
}

/** A share as "79%". */
export function formatPercent(share: number | null | undefined): string {
  return share == null ? NONE : `${Math.round(PERCENT * share)}%`;
}

/** Degrees as "0.48°". */
export function formatDegrees(deg: number | null | undefined, digits = 2): string {
  return deg == null ? NONE : `${deg.toFixed(digits)}°`;
}

/** A speed as "480 °/s". */
export function formatSpeed(degPerSecond: number | null | undefined): string {
  return degPerSecond == null ? NONE : `${Math.round(degPerSecond)} °/s`;
}

/**
 * How far one time is from another, from seconds (negative: before): "at the same time",
 * "12 s after", "3 min before", "2 h after", "4 days before".
 */
export function formatOffset(seconds: number): string {
  const apart = Math.abs(seconds);
  if (apart < 1) return 'at the same time';
  const days = Math.round(apart / SECONDS_PER_DAY);
  const size =
    apart < SECONDS_PER_MINUTE
      ? `${Math.round(apart)} s`
      : apart < SECONDS_PER_HOUR
        ? `${Math.round(apart / SECONDS_PER_MINUTE)} min`
        : apart < SECONDS_PER_DAY
          ? `${Math.round(apart / SECONDS_PER_HOUR)} h`
          : `${days} ${days === 1 ? 'day' : 'days'}`;
  return `${size} ${seconds < 0 ? 'before' : 'after'}`;
}

/** A count, or a dash when there is none. */
export function formatCount(value: number | null | undefined): string {
  return value == null ? NONE : formatNumber(value);
}

/** The arrows for the eight directions, from right round to down-right, each 45 degrees on from the one before. */
const ARROWS = '→↗↑↖←↙↓↘';
/** Degrees in a full turn. */
const FULL_TURN_DEG = 360;
/** Degrees between one arrow's direction and the next: 45. */
const ARROW_STEP_DEG = FULL_TURN_DEG / ARROWS.length;

/** A flick's direction in degrees (0 right, 90 up) as one of eight arrows. */
export function arrow(direction: number): string {
  const angleDeg = ((direction % FULL_TURN_DEG) + FULL_TURN_DEG) % FULL_TURN_DEG;
  return ARROWS[Math.round(angleDeg / ARROW_STEP_DEG) % ARROWS.length];
}

/** Each of the core's eight direction names as its arrow. */
export const DIRECTION_ARROWS: Record<Direction, string> = {
  right: '→',
  'up-right': '↗',
  up: '↑',
  'up-left': '↖',
  left: '←',
  'down-left': '↙',
  down: '↓',
  'down-right': '↘',
};

/** How a flick landed: on the target, short of it, or past its far edge. */
export type LandingKind = 'on target' | 'underflick' | 'overflick';

/** Where a flick landed: its kind, and the degrees short of the target or past its far edge (null on target). */
export interface Landing {
  /** On target, an underflick or an overflick. */
  kind: LandingKind;
  /** The degrees still to go (an underflick) or past the far edge (an overflick); null on target. */
  degrees: number | null;
}

/**
 * Where a flick landed, against a target of radius `radiusDeg`. `endLeft` is how far along the way
 * to the target was left when the flick ended, in degrees (below 0: past it).
 */
export function landing(endLeft: number, radiusDeg: number): Landing {
  if (endLeft > radiusDeg) return { kind: 'underflick', degrees: endLeft };
  if (endLeft < -radiusDeg) return { kind: 'overflick', degrees: -endLeft - radiusDeg };
  return { kind: 'on target', degrees: null };
}

/**
 * Where a flick landed, in words: on target, an underflick (the degrees still to go) or an
 * overflick (the degrees past the far edge); see `landing`.
 */
export function formatEnded(endLeft: number, radiusDeg: number): string {
  const { kind, degrees } = landing(endLeft, radiusDeg);
  return degrees === null ? kind : `${kind} ${degrees.toFixed(1)}°`;
}
