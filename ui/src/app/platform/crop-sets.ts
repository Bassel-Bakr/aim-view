import { ResourceRef } from '@angular/core';
import { CropAnswer, CropAnswers, CropEntry, CropPage } from '../api';

/** A set's answers by crop id. */
export type CropAnswerMap = Record<string, CropAnswer>;

/** What an import of answers did: written, kept (the folder held a newer answer), and of crops it does not have. */
export interface CropImport {
  written: number;
  kept: number;
  unknown: number;
}

/**
 * The check folders of detector crops and the user's answer to each crop, for the Crops page
 * (service/src/crops.rs). A check folder is what python/model/crop_check/make_page.py writes: its crops, their
 * pictures, and the sets they belong to. The review server reads test_out/vod_model/check_*; browser mode and the
 * desktop app keep the folders the user adds in their data folder. Answers move between modes as one document: an
 * export from browser mode is imported on the review server. Each mode provides one (modes/mode.*.ts).
 */
export abstract class CropSets {
  /** Every check folder and its sets. Call it where a resource can be made. */
  abstract pages(): ResourceRef<CropPage[] | undefined>;

  /** A set's crops, in the folder's order. */
  abstract crops(
    page: () => string | undefined,
    set: () => string | undefined,
  ): ResourceRef<CropEntry[] | undefined>;

  /** A set's answers by crop id. */
  abstract answers(
    page: () => string | undefined,
    set: () => string | undefined,
  ): ResourceRef<CropAnswerMap | undefined>;

  /** A crop's picture (PNG, 256 x 256). */
  abstract image(page: string, id: string): Promise<Blob>;

  /** Keeps a crop's answer; resolves to it as kept. */
  abstract save(page: string, id: string, answer: CropAnswer): Promise<CropAnswer>;

  /** Every answer of a check folder, as the document an import takes. */
  abstract exportAnswers(page: string): Promise<CropAnswers>;

  /** Takes a document an export gave: a crop's answer is kept unless the folder holds a newer one. */
  abstract importAnswers(page: string, document: CropAnswers): Promise<CropImport>;

  /**
   * Adds a check folder the user chose (its files, as a folder input gives them), copied into this browser; resolves
   * to its name. Null where the folders are read where they are (the review server).
   */
  abstract readonly addFolder: ((files: readonly File[]) => Promise<string>) | null;
}
