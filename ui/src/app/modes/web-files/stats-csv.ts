/**
 * KovaaK's stats files, read in the browser (as python/retired/review.py `load_stats` read them),
 * and paired with recordings by their names. In: .csv files the user adds with videos. Out: which
 * ones are stats files, and the stats file for each video, for an upload (server-recordings.ts) or
 * an added recording (browser-recordings.ts).
 */

import { StatsSummary } from '../../api';

/** A stats file's key-value lines ("Score:,558.46"), by key. */
export type StatsMeta = Record<string, string>;

/** A stats file: its name, its key-value lines, and how many kill rows its first table holds. */
export interface StatsCsv {
  /** The file's name, which holds its scenario and when the run ended. */
  name: string;
  /** Its "key:,value" lines, by key. */
  meta: StatsMeta;
  /** The kills its first table lists, one row each. */
  killRows: number;
  /** The whole file, which the review reads. */
  text: string;
}

/** A stats file's name: "<scenario> - Challenge - <stamp> Stats.csv". */
const STATS_NAME = /^(.+) - Challenge - (\d{4}\.\d\d\.\d\d-\d\d\.\d\d\.\d\d) Stats\.csv$/;
/** A recording's name as KovOBS writes it: "<scenario> - <score> - <stamp>.<ext>". */
const VOD_NAME = /^(.+) - ([-\d.]+) - (\d{4}\.\d\d\.\d\d-\d\d\.\d\d\.\d\d)\.\w+$/;
/** A file-name time stamp, yyyy.mm.dd-hh.mm.ss, each part captured. */
const STAMP = /^(\d{4})\.(\d\d)\.(\d\d)-(\d\d)\.(\d\d)\.(\d\d)$/;
/** A stats file and a recording this many seconds apart or less are the same run. */
const SAME_RUN_S = 5;
/** Milliseconds in a second, to turn Date.UTC's milliseconds into seconds. */
const MS_PER_SECOND = 1000;

/**
 * Reads a stats file: every "key:,value" line, and the kill rows (the first table, up to its blank
 * line; later tables can also start with a digit). Null when it is not one of KovaaK's stats files
 * (no Scenario line).
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

/**
 * What the page shows of a stats file: its scenario, score, kills (the Kills line, else the kill
 * rows), accuracy (hits over shots; null without both counts or with no shots) and the time stamp
 * in its name.
 */
export function statsSummary(stats: StatsCsv): StatsSummary {
  const metaNumber = (key: string): number | null => {
    const value = Number.parseFloat(stats.meta[key] ?? '');
    return Number.isFinite(value) ? value : null;
  };
  const hits = metaNumber('Hit Count');
  const misses = metaNumber('Miss Count');
  const shots = (hits ?? 0) + (misses ?? 0);
  return {
    scenario: stats.meta['Scenario'] || null,
    score: metaNumber('Score'),
    kills: metaNumber('Kills') ?? stats.killRows,
    accuracy: hits !== null && misses !== null && shots > 0 ? hits / shots : null,
    stamp: STATS_NAME.exec(stats.name)?.[2] ?? null,
  };
}

/** A recording's name as KovOBS writes it: "<scenario> - <score> - <time>.mp4". */
export interface VodName {
  /** The scenario's name, as KovaaK names it. */
  scenario: string;
  /** The run's score, as KovOBS wrote it in the name. */
  score: number;
  /** When the run ended: yyyy.mm.dd-hh.mm.ss. */
  stamp: string;
}

/** A recording's scenario, score and time stamp from its name; null when KovOBS did not name it. */
export function parseVodName(name: string): VodName | null {
  const match = VOD_NAME.exec(name);
  return match ? { scenario: match[1], score: Number(match[2]), stamp: match[3] } : null;
}

/** A video's name of a title and a time stamp, as a recording added from a link is named. */
export interface TitledName {
  /** The video's title on its site. */
  title: string;
  /** When the video was uploaded to its site (else when it was added): yyyy.mm.dd-hh.mm.ss. */
  stamp: string;
}

/**
 * "<title> - <stamp>.<ext>" (a link's name: the review server names it so): the title and the
 * stamp; null for any other name.
 */
export function parseTitledName(name: string): TitledName | null {
  const match = /^(.+) - (\d{4}\.\d{2}\.\d{2}-\d{2}\.\d{2}\.\d{2})\.\w+$/.exec(name);
  return match && stampSeconds(match[2]) !== null ? { title: match[1], stamp: match[2] } : null;
}

/**
 * A file-name time stamp as seconds on one clock (both sides use the same one), or null when it is
 * not one. The year 0026 reads as 2026.
 */
export function stampSeconds(stamp: string): number | null {
  const match = STAMP.exec(stamp);
  if (!match) return null;
  const year = Number(match[1].startsWith('00') ? `20${match[1].slice(2)}` : match[1]);
  const [, , month, day, hour, minute, second] = match.map(Number);
  return Date.UTC(year, month - 1, day, hour, minute, second) / MS_PER_SECOND;
}

/**
 * The stats file for a video among files: the one named for the same scenario within
 * `SAME_RUN_S` seconds of the video's time; else, when onlyPair says the video came alone and there
 * is one stats file, that one. Null when none fits.
 */
export function statsForVideo(
  video: string,
  files: readonly StatsCsv[],
  onlyPair: boolean,
): StatsCsv | null {
  const vod = parseVodName(video);
  const videoSeconds = vod && stampSeconds(vod.stamp);
  const named = files.find((file) => {
    const match = STATS_NAME.exec(file.name);
    const statsSeconds = match && stampSeconds(match[2]);
    return (
      vod !== null &&
      videoSeconds !== null &&
      statsSeconds !== null &&
      match?.[1].toLowerCase() === vod.scenario.toLowerCase() &&
      Math.abs(statsSeconds - videoSeconds) <= SAME_RUN_S
    );
  });
  return named ?? (onlyPair && files.length === 1 ? files[0] : null);
}
