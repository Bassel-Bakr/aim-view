import { Kind } from './api';

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
