/**
 * The StoredData contract: what the app keeps, with each part's size, and removing a part the user
 * can do without. In: each mode's implementation (modes/mode.*.ts; service/src/library/usage.rs
 * answers it). Out: the top bar's Storage panel (storage-panel/).
 */

import { ResourceRef } from '@angular/core';

/** What a part of the kept data is. */
export type KeptKind =
  'reviews' | 'marks' | 'cutoff' | 'kovaak' | 'uploads' | 'mouse' | 'old_files' | 'ffmpeg';

/**
 * One part of the kept data: its id (what removing it names), its kind, its size, whether it can
 * be removed here, and the counts its kind gives.
 */
export interface KeptPart {
  /** The part's id: review:<model> for a model's reviews, else its kind. */
  id: string;
  /** What it is. */
  kind: KeptKind;
  /** Its size in bytes (the reviews compressed, as the database keeps them). */
  bytes: number;
  /** Whether the user can remove it here; parts the user made are only listed. */
  removable: boolean;
  /** The model that made the reviews ("" for reviews from before they were kept per model). */
  model?: string;
  /** How many recordings the reviews cover. */
  recordings?: number;
  /** Whether the model is still offered (models.json lists it). */
  listed?: boolean;
  /** How many of KovaaK's stats files are kept. */
  stats?: number;
  /** How many of KovaaK's scenario files are kept. */
  scenarios?: number;
  /** How many files a folder holds. */
  files?: number;
}

/** Everything the app keeps: its total size, the database's when there is one, and each part. */
export interface KeptData {
  /** Everything's size in bytes. */
  total: number;
  /** The database file's size in bytes; null when the data are kept as files. */
  database: number | null;
  /** Each part. */
  parts: KeptPart[];
}

/** What the app keeps and how much space it takes, and removing what the user can do without. */
export abstract class StoredData {
  /** What is kept, read when the resource is made. Call it where a resource can be made. */
  abstract kept(): ResourceRef<KeptData | undefined>;

  /** Removes a removable part (and gives the space back); resolves to what is kept then. */
  abstract remove(id: string): Promise<KeptData>;
}
