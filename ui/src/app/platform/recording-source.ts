import { Signal, WritableSignal } from '@angular/core';
import { LinkInfo, Recording } from '../api';

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

/**
 * A video added from a link, on its way: what is being done (downloading it, copying it into this browser), and the
 * megabytes done of total (total 0 while it is not known).
 */
export interface VideoDownloading {
  state: 'downloading';
  label: string;
  done: number;
  total: number;
}

/** A video added from a link that could not be downloaded: why, in plain words. */
export interface VideoNotDownloaded {
  state: 'not-downloaded';
  error: string;
}

export type VideoState =
  VideoRemuxing | VideoReady | VideoFailed | VideoDownloading | VideoNotDownloaded;

/** What adding files did: the recordings added, and the .csv files that are not KovaaK's stats files. */
export interface AddResult {
  ids: string[];
  notStats: string[];
}

/** Files being prepared or sent, for the top bar: what, and the share done (null while it is not known). */
export interface Transfer {
  label: string;
  share: number | null;
  /** How many items are done, of how many, where it goes item by item (files read, recordings paired). */
  count?: ItemCount;
}

/** How many items are done, of how many. */
export interface ItemCount {
  done: number;
  total: number;
}

/**
 * A folder of recordings the user can open: what its button says and why, whether it is being read, the action (run
 * in the click), and files: the folder chosen as files instead (a folder input), where the browser cannot open it.
 */
export interface FolderAction {
  label: string;
  detail: string;
  busy: boolean;
  run: () => Promise<void>;
  files: ((files: File[]) => Promise<void>) | null;
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
  /** A folder of recordings to open; null where the mode lists its own (the review server's library). */
  abstract readonly folder: Signal<FolderAction | null>;
  /** Whether the list can be cleared (recordings opened in this browser); the server's library cannot. */
  abstract readonly clearable: boolean;
  /**
   * The address of the Aim View server on this computer that downloads links for this browser, which the user can
   * change; null where the mode downloads them itself.
   */
  abstract readonly linkServer: WritableSignal<string> | null;

  /** A recording's video, or null when the recording is not one of these. */
  abstract video(id: string): VideoState | null;
  /** Whether the id still opens the recording after the page loads again (so a link can hold it). */
  abstract lasting(id: string): boolean;
  /** Adds recordings from this computer, each with its stats .csv when one is among the files. */
  abstract add(files: readonly File[]): Promise<AddResult>;
  /**
   * What a link offers (a video's page on YouTube, Twitch, Medal and the other sites yt-dlp reads, or a video file's
   * address): its title and the qualities to choose from, best first; none for a plain video file.
   */
  abstract linkInfo(url: string): Promise<LinkInfo>;
  /**
   * Adds a recording from a link in the chosen quality (a format's id from linkInfo; null: the best). It resolves
   * with the new recording's id as soon as the download starts: the recording is listed at once, and its video is
   * downloading until it is in.
   */
  abstract addLink(url: string, format: string | null): Promise<string>;

  /** Cancels a link's download while it runs: the recording is left not downloaded ("Cancelled"). */
  abstract cancelLink(id: string): Promise<void>;
  /** Changes a recording's row after a change made elsewhere (it was reviewed, it has a stats file). */
  abstract patch(id: string, change: Partial<Recording>): void;
  /** Empties the list, and forgets the recordings folder: the files themselves stay where they are. */
  abstract clear(): Promise<void>;
}
