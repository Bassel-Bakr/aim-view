import { Direction, Kind } from './api';

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

export const KIND_LABELS: Record<Kind, string> = {
  static: 'Static',
  dynamic: 'Dynamic',
  tracking: 'Tracking',
  switching: 'Switching',
};

/** 13278 as "13,278", 889.26 as "889.26". */
export function formatNumber(v: number): string {
  return v.toLocaleString('en-US', { maximumFractionDigits: 2 });
}

/**
 * A recording's time stamp ("2026.08.11-13.55.27") as "Aug 11, 13:55", with the year when it is not this one. Some
 * KovOBS recordings from June 2026 carry the year 0026.
 */
export function formatStamp(stamp: string, thisYear = new Date().getFullYear()): string {
  const m = /^(\d{4})\.(\d\d)\.(\d\d)-(\d\d)\.(\d\d)/.exec(stamp);
  if (!m) return stamp;
  const year = m[1].startsWith('00') ? `20${m[1].slice(2)}` : m[1];
  const day = `${MONTHS[Number(m[2]) - 1]} ${Number(m[3])}`;
  return `${day}${Number(year) === thisYear ? '' : ` ${year}`}, ${m[4]}:${m[5]}`;
}

/** Bytes as "68 MB". */
export function formatSize(bytes: number): string {
  return `${Math.round(bytes / 1e6)} MB`;
}

const NONE = '–';

/** Seconds as "425 ms". */
export function formatMs(seconds: number | null | undefined): string {
  return seconds == null ? NONE : `${Math.round(1000 * seconds)} ms`;
}

/** Seconds as milliseconds under a second ("167 ms"), else seconds ("1.25 s"). */
export function formatSeconds(seconds: number | null | undefined): string {
  if (seconds == null) return NONE;
  return seconds < 1 ? formatMs(seconds) : `${seconds.toFixed(2)} s`;
}

/** A share as "79%". */
export function formatPercent(share: number | null | undefined): string {
  return share == null ? NONE : `${Math.round(100 * share)}%`;
}

/** Degrees as "0.48°". */
export function formatDegrees(deg: number | null | undefined, digits = 2): string {
  return deg == null ? NONE : `${deg.toFixed(digits)}°`;
}

/** A speed as "480 °/s". */
export function formatSpeed(degPerSecond: number | null | undefined): string {
  return degPerSecond == null ? NONE : `${Math.round(degPerSecond)} °/s`;
}

/** How far one time is from another: "at the same time", "12 s after", "3 min before", "2 h after", "4 days before". */
export function formatOffset(seconds: number): string {
  const a = Math.abs(seconds);
  if (a < 1) return 'at the same time';
  const days = Math.round(a / 86400);
  const size =
    a < 60
      ? `${Math.round(a)} s`
      : a < 3600
        ? `${Math.round(a / 60)} min`
        : a < 86400
          ? `${Math.round(a / 3600)} h`
          : `${days} ${days === 1 ? 'day' : 'days'}`;
  return `${size} ${seconds < 0 ? 'before' : 'after'}`;
}

/** A count, or a dash when there is none. */
export function formatCount(v: number | null | undefined): string {
  return v == null ? NONE : formatNumber(v);
}

/** A flick's direction in degrees (0 right, 90 up) as one of eight arrows. */
export function arrow(direction: number): string {
  const a = ((direction % 360) + 360) % 360;
  return '→↗↑↖←↙↓↘'[Math.round(a / 45) % 8];
}

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

/** Where a flick's main movement stopped, against a target of radius r: short of it, past it, or on it. */
export function formatEnded(endLeft: number, r: number): string {
  if (endLeft > r) return `short, ${endLeft.toFixed(1)}° to go`;
  if (endLeft < -r) return `past, by ${(-endLeft - r).toFixed(1)}°`;
  return 'on the target';
}
