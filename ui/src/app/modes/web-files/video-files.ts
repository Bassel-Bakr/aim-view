/**
 * Sorts the files the user adds by their names, and makes a video an MP4 the browser can play. In:
 * the File objects from a file picker, a drop or the VODs folder. Out: which ones are videos and
 * stats .csv files, and each video as an MP4 Blob (remux.ts loads only when one needs it).
 */

/** The video file names the app takes, by extension. */
const VIDEO = /\.(mp4|mkv|mov|webm)$/i;
/** A .csv file name, which may be a stats file. */
const CSV = /\.csv$/i;
/** An .mp4 file name, which every browser plays as it is. */
const MP4 = /\.mp4$/i;

/** Whether the file is a video the app takes (MP4, MKV, MOV or WebM), by its name. */
export function isVideo(file: File): boolean {
  return isVideoName(file.name);
}

/** Whether the name ends in a video extension the app takes. */
function isVideoName(name: string): boolean {
  return VIDEO.test(name);
}

/** Whether the file is a .csv, by its name: it may be a stats file (stats-csv.ts reads it). */
export function isCsv(file: File): boolean {
  return CSV.test(file.name);
}

/** Whether the browser plays the file as it is: an MP4, with no remux. */
export function isMp4(file: File): boolean {
  return MP4.test(file.name);
}

/** The file's name as an MP4's. */
export function mp4Name(file: File): string {
  return file.name.replace(/\.\w+$/, '.mp4');
}

/**
 * The video as an MP4: the file itself, or remuxed in the browser (remuxToMp4). progress gets the
 * share done. The remux code loads only the first time a video needs it.
 */
export async function toMp4(file: File, progress: (share: number) => void): Promise<Blob> {
  if (isMp4(file)) return file;
  const { remuxToMp4 } = await import('./remux');
  return remuxToMp4(file, progress);
}
