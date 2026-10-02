import { Signal } from '@angular/core';
import { Recording } from '../api';

/** A video being remuxed into MP4 so the browser can play it; progress is the share done, 0 to 1. */
export interface VideoRemuxing {
  state: 'remuxing';
  progress: number;
}

/** A video ready to play from url; remuxed when it was remuxed into MP4 in the browser. */
export interface VideoReady {
  state: 'ready';
  url: string;
  remuxed: boolean;
}

/** A video the remux failed on: it plays from the file as it is, if the browser can play it. */
export interface VideoFailed {
  state: 'failed';
  url: string;
  error: string;
}

export type VideoState = VideoRemuxing | VideoReady | VideoFailed;

/** What adding files did: the recordings added, and the .csv files that are not KovaaK's stats files. */
export interface AddResult {
  ids: string[];
  notStats: string[];
}

/** Files being prepared or sent, for the top bar: what, and the share done (null while it is not known). */
export interface Transfer {
  label: string;
  share: number | null;
}

/**
 * Where the recordings come from: the review server, files opened in the browser, or the disk (the desktop app).
 * Each mode provides one (modes/mode.*.ts).
 */
export abstract class RecordingSource {
  /** The recordings, newest first. */
  abstract readonly recordings: Signal<Recording[]>;
  /** The list is loading for the first time. */
  abstract readonly loading: Signal<boolean>;
  /** Why the list could not be read, in words; null when it was. */
  abstract readonly problem: Signal<string | null>;
  /** Where added files go, in words (the upload button and the empty page say it). */
  abstract readonly addedFilesGo: string;
  /** Files being prepared or sent; null when there are none. */
  abstract readonly transfer: Signal<Transfer | null>;

  /** A recording's video, or null when the recording is not one of these. */
  abstract video(id: string): VideoState | null;
  /** Whether the id still opens the recording after the page loads again (so a link can hold it). */
  abstract lasting(id: string): boolean;
  /** Adds recordings from this computer, each with its stats .csv when one is among the files. */
  abstract add(files: readonly File[]): Promise<AddResult>;
  /** Changes a recording's row after a change made elsewhere (it was reviewed, it has a stats file). */
  abstract patch(id: string, change: Partial<Recording>): void;
}
