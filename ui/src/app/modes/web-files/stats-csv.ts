/** KovaaK's stats files, read in the browser (as review.load_stats reads them). */

import { StatsSummary } from '../../api';

/** A stats file's key-value lines ("Score:,558.46"), by key. */
export type StatsMeta = Record<string, string>;

/** A stats file: its name, its key-value lines, and how many kill rows its first table holds. */
export interface StatsCsv {
  name: string;
  meta: StatsMeta;
  killRows: number;
  /** The whole file, which the review reads. */
  text: string;
}

const STATS_NAME = /^(.+) - Challenge - (\d{4}\.\d\d\.\d\d-\d\d\.\d\d\.\d\d) Stats\.csv$/;
const VOD_NAME = /^(.+) - ([-\d.]+) - (\d{4}\.\d\d\.\d\d-\d\d\.\d\d\.\d\d)\.\w+$/;
const STAMP = /^(\d{4})\.(\d\d)\.(\d\d)-(\d\d)\.(\d\d)\.(\d\d)$/;
/** A stats file and a recording this many seconds apart or less are the same run. */
const SAME_RUN_S = 5;

/**
 * Reads a stats file: every "key:,value" line, and the kill rows (the first table, up to its blank line; later tables
 * can also start with a digit). Null when it is not one of KovaaK's stats files (no Scenario line).
 */
export function parseStatsCsv(name: string, text: string): StatsCsv | null {
  const lines = text.split(/\r?\n/);
  const meta: StatsMeta = {};
  for (const line of lines) {
    const at = line.indexOf(':,');
    if (at >= 0) meta[line.slice(0, at)] = line.slice(at + 2);
  }
  let killRows = 0;
  for (const line of lines.slice(1)) {
    if (!line.trim()) break;
    if (/^\d/.test(line)) killRows++;
  }
  return 'Scenario' in meta ? { name, meta, killRows, text } : null;
}

/** Reads a .csv file as a stats file, or null when it is not one. */
export async function readStats(file: File): Promise<StatsCsv | null> {
  return parseStatsCsv(file.name, await file.text());
}

export function statsSummary(s: StatsCsv): StatsSummary {
  const num = (key: string): number | null => {
    const v = Number.parseFloat(s.meta[key] ?? '');
    return Number.isFinite(v) ? v : null;
  };
  const hits = num('Hit Count');
  const misses = num('Miss Count');
  const shots = (hits ?? 0) + (misses ?? 0);
  return {
    scenario: s.meta['Scenario'] || null,
    score: num('Score'),
    kills: num('Kills') ?? s.killRows,
    accuracy: hits !== null && misses !== null && shots > 0 ? hits / shots : null,
    stamp: STATS_NAME.exec(s.name)?.[2] ?? null,
  };
}

/** A recording's name as KovOBS writes it: "<scenario> - <score> - <time>.mp4". */
export interface VodName {
  scenario: string;
  score: number;
  stamp: string;
}

export function parseVodName(name: string): VodName | null {
  const m = VOD_NAME.exec(name);
  return m ? { scenario: m[1], score: Number(m[2]), stamp: m[3] } : null;
}

/** A video's name of a title and a time stamp, as a recording added from a link is named. */
export interface TitledName {
  title: string;
  stamp: string;
}

/** "<title> - <stamp>.<ext>" (a link's name: the review server names it so): the title and the stamp. */
export function parseTitledName(name: string): TitledName | null {
  const m = /^(.+) - (\d{4}\.\d{2}\.\d{2}-\d{2}\.\d{2}\.\d{2})\.\w+$/.exec(name);
  return m && stampSeconds(m[2]) !== null ? { title: m[1], stamp: m[2] } : null;
}

/** A file-name time stamp as seconds on one clock (both sides use the same one). The year 0026 reads as 2026. */
export function stampSeconds(stamp: string): number | null {
  const m = STAMP.exec(stamp);
  if (!m) return null;
  const year = Number(m[1].startsWith('00') ? `20${m[1].slice(2)}` : m[1]);
  const [, , mo, d, h, mi, s] = m.map(Number);
  return Date.UTC(year, mo - 1, d, h, mi, s) / 1000;
}

/**
 * The stats file for a video among files: the one named for the same scenario within five seconds of the video's
 * time; else, when there is one video and one stats file, that one.
 */
export function statsForVideo(
  video: string,
  files: readonly StatsCsv[],
  onlyPair: boolean,
): StatsCsv | null {
  const vod = parseVodName(video);
  const t = vod && stampSeconds(vod.stamp);
  const named = files.find((f) => {
    const m = STATS_NAME.exec(f.name);
    const ts = m && stampSeconds(m[2]);
    return (
      vod !== null &&
      t !== null &&
      ts !== null &&
      m?.[1].toLowerCase() === vod.scenario.toLowerCase() &&
      Math.abs(ts - t) <= SAME_RUN_S
    );
  });
  return named ?? (onlyPair && files.length === 1 ? files[0] : null);
}
