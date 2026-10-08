/**
 * The Exports contract: recordings shared as one zip, and a zip opened. In: each mode's
 * implementation (modes/mode.*.ts; service/src/library/export.rs gives each recording's files).
 * Out: the Storage panel's Share section (storage-panel/).
 */

/** What an export or an opening has done, 0 to 1. */
export type ShareProgress = (share: number) => void;

/** What opening an export did: the recordings added, and those it could not add, with why. */
export interface OpenedExport {
  /** The ids of the recordings added (as uploads), with their reviews. */
  added: string[];
  /** The recordings left out, each with why. */
  skipped: string[];
}

/**
 * Recordings shared as one zip (docs/storage-design.md, "Export"): the reviews, the marks, the
 * stats files and the scenarios' facts, and the videos when asked. An export opened adds each of
 * its recordings that has its video as an upload, with its stats file, reviews and marks. Each mode
 * provides one (modes/mode.*.ts).
 */
export abstract class Exports {
  /**
   * Writes the recordings into one zip the user saves: where they choose (the save dialog) when
   * the browser lets the page, else as a download. Call it from a click: the dialog needs one.
   */
  abstract export(ids: readonly string[], videos: boolean, progress: ShareProgress): Promise<void>;

  /** Adds the recordings of an export (a zip one made). */
  abstract open(zip: File, progress: ShareProgress): Promise<OpenedExport>;
}
