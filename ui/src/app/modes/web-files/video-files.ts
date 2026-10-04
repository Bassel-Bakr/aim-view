const VIDEO = /\.(mp4|mkv|mov|webm)$/i;
const CSV = /\.csv$/i;
const MP4 = /\.mp4$/i;

export function isVideo(file: File): boolean {
  return isVideoName(file.name);
}

function isVideoName(name: string): boolean {
  return VIDEO.test(name);
}

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
 * The video as an MP4: the file itself, or remuxed in the browser (remuxToMp4). progress gets the share done. The
 * remux code loads only the first time a video needs it.
 */
export async function toMp4(file: File, progress: (share: number) => void): Promise<Blob> {
  if (isMp4(file)) return file;
  const { remuxToMp4 } = await import('./remux');
  return remuxToMp4(file, progress);
}
