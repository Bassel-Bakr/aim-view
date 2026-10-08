/**
 * KovaaK's files sent to the review service in batches, each file read once (POST
 * /api/kovaak_files, service/src/library/browser.rs): the service keeps each stats file's run and
 * each scenario file's facts in its database, not the files. In: the files (their paths in /kovaak)
 * and what the service keeps (GET /api/kovaak_files). Out: the batches, only for files new or
 * changed since they were sent.
 */
import { encodeBatch } from '../web-files/file-batch';
import { ChosenFile } from './service-messages';

/** The most files in one batch. */
const BATCH_FILES = 1000;
/** The most bytes in one batch (a batch holds at least one file). */
const BATCH_BYTES = 8 * 1024 * 1024;
/** Milliseconds in a second: a File's time is in ms, the service's in seconds. */
const MS_PER_SECOND = 1000;
/** Where stats files are in /kovaak. */
const STATS = 'stats/';

/** A file the service keeps: its name (stats) or path (scenarios), size and time in seconds. */
export type KeptFile = [string, number, number];

/** What the service keeps of KovaaK's files (GET /api/kovaak_files). */
export interface KovaakKept {
  /** Every stats file, by name. */
  stats: KeptFile[];
  /** Every scenario file, by its path in /kovaak. */
  scenarios: KeptFile[];
}

/** Says how many of the files are sent. */
export type BatchProgress = (done: number, total: number) => void;

/** Sends one batch's bytes to the service. */
export type BatchPost = (body: Uint8Array) => Promise<unknown>;

/** A file's time as the service keeps it: seconds since 1970. */
function seconds(file: File): number {
  return file.lastModified / MS_PER_SECOND;
}

/** The files the service does not keep as they are now: new, or of another size or time. */
export function freshFiles(files: readonly ChosenFile[], kept: KovaakKept): ChosenFile[] {
  const known = new Map<string, KeptFile>();
  for (const row of kept.stats) known.set(STATS + row[0], row);
  for (const row of kept.scenarios) known.set(row[0], row);
  return files.filter(({ path, file }) => {
    const row = known.get(path);
    return !row || row[1] !== file.size || row[2] !== seconds(file);
  });
}

/** Sends the files in batches through `post`, one after another; resolves to how many it sent. */
export async function sendBatches(
  files: readonly ChosenFile[],
  post: BatchPost,
  progress: BatchProgress,
): Promise<number> {
  let done = 0;
  while (done < files.length) {
    progress(done, files.length);
    const batch: ChosenFile[] = [];
    let bytes = 0;
    for (const chosen of files.slice(done)) {
      const full = batch.length >= BATCH_FILES || bytes + chosen.file.size > BATCH_BYTES;
      if (batch.length && full) break;
      batch.push(chosen);
      bytes += chosen.file.size;
    }
    await post(await encodeBatch(batch));
    done += batch.length;
  }
  progress(files.length, files.length);
  return files.length;
}
