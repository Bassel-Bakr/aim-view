import { Signal } from '@angular/core';

/** What loading files did: the area examples and area types read, and the files it could not use (with why). */
export interface ExamplesLoaded {
  examples: number;
  kinds: number;
  refused: string[];
}

/** The area finder's training data kept here: how many examples, from how many recordings, and how many area types. */
export interface ExamplesCount {
  examples: number;
  recordings: number;
  kinds: number;
}

/**
 * The area finder's training data where the browser keeps it: the examples saved areas make, and the area types they
 * name. The user downloads them as the review server's files (area_examples.jsonl, area_kinds.json), and loads those
 * files to start from what they labelled there.
 */
export interface ExamplesStore {
  readonly count: Signal<ExamplesCount>;
  /** The names of the review server's files it downloads as: area_examples.jsonl and area_kinds.json. */
  readonly fileNames: readonly string[];
  /** One of those files, written as the review server writes it. */
  file(name: string): Blob;
  /** Loads area_examples.jsonl and area_kinds.json: what they hold replaces what is kept of the same recordings and types. */
  load(files: readonly File[]): Promise<ExamplesLoaded>;
}

/**
 * Labelling the areas of recordings: the queue of recordings to label, skipping one, and the mark for a recording of
 * another game. Each mode provides one (modes/mode.*.ts).
 */
export abstract class Labelling {
  /**
   * The recordings to label areas in, in order: those added from this computer first (other players' layouts), then
   * the most recent recording of each scenario. It leaves out probes (the view hardly moves in them), other games,
   * skipped recordings and those with saved areas.
   */
  abstract queue(): Promise<string[]>;
  /** Leaves the recording out of the queue from now on. */
  abstract skip(id: string): Promise<void>;
  /**
   * Marks the recording as another game, not an aim trainer (on), or as an aim trainer again. A marked recording
   * leaves the queues and what the area finder learns. Its row in the recordings list follows.
   */
  abstract setNotAim(id: string, on: boolean): Promise<void>;
  /**
   * The area finder's training data, to download and to load; null where the review server keeps it itself
   * (test_out/vod_app/area_examples.jsonl, written when areas are saved).
   */
  abstract readonly examples: ExamplesStore | null;
}
