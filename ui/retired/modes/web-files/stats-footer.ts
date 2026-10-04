/** A stats file's footer: its "Key:,value" lines, read from the end of the file (as service/src/library/stats.rs). */

import { PastRun } from '../../platform/score-history';
import { StatsMeta } from './stats-csv';

/** The end of a stats file read for its key-value lines, in bytes (they take about 1.5 kB). */
export const FOOTER_BYTES = 4096;

/** The key-value lines of a text ("Score:,558.46"), by key; a later line wins. */
export function footerMeta(text: string): StatsMeta {
  const meta: StatsMeta = {};
  for (const line of text.split(/\r\n|\n|\r/)) {
    const at = line.indexOf(':,');
    if (at >= 0) meta[line.slice(0, at)] = line.slice(at + 2);
  }
  return meta;
}

/** A stats file's key-value lines, from its last few kB, or the whole file when they hold no score. */
export async function readFooter(file: Blob): Promise<StatsMeta> {
  const from = Math.max(0, file.size - FOOTER_BYTES);
  const text = await file.slice(from).text();
  // the first line is cut unless the file starts there
  const cut = text.indexOf('\n');
  const meta = footerMeta(from === 0 ? text : cut < 0 ? '' : text.slice(cut + 1));
  return from > 0 && !('Score' in meta) ? footerMeta(await file.text()) : meta;
}

/** A number from a key-value line, or null. */
function footerNumber(meta: StatsMeta, key: string): number | null {
  const text = meta[key]?.trim();
  const v = text ? Number(text) : Number.NaN;
  return Number.isFinite(v) ? v : null;
}

/** The run a stats file's key-value lines describe; null without a score. Accuracy is hits over shots. */
export function pastRun(stamp: string, meta: StatsMeta): PastRun | null {
  const score = footerNumber(meta, 'Score');
  if (score === null) return null;
  const hits = footerNumber(meta, 'Hit Count');
  const misses = footerNumber(meta, 'Miss Count');
  const accuracy =
    hits !== null && misses !== null && hits + misses > 0 ? hits / (hits + misses) : null;
  return { stamp, score, kills: footerNumber(meta, 'Kills'), accuracy };
}
