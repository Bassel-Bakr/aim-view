import { ResourceRef, Signal } from '@angular/core';
import { FaintChoice, FaintSetting, Job } from '../api';

/** The cut-off labels kept here: how many crops, from how many recordings. */
export interface CutoffLabelsCount {
  crops: number;
  recordings: number;
}

/**
 * The detector labels submitted cut-offs wrote, where the browser keeps them. The user downloads them as the files
 * training reads: a zip of checked.jsonl and train/*.npz, as the review server writes them in
 * test_out/vod_model/hand/cutoff/.
 */
export interface CutoffLabelsStore {
  readonly count: Signal<CutoffLabelsCount>;
  /** The name it downloads as. */
  readonly fileName: string;
  /** The labels as that zip. */
  file(): Promise<Blob>;
}

/**
 * The faint-target cut-off of each recording: the user's setting (leave out the tracks the detector is far less sure of
 * than of the recording's targets: wall seams and tiles), which a tracking run's measures use; a submitted cut-off,
 * written as detector labels; and the queue of recordings to set one in. Each mode provides one (modes/mode.*.ts).
 */
export abstract class FaintCutoffs {
  /** The recording's cut-off: off at 0.3 when none is saved. Call it where a resource can be made. */
  abstract setting(id: () => string | undefined): ResourceRef<FaintSetting | undefined>;

  /**
   * Keeps the recording's cut-off (the offset from 0.2 to 0.6). A tracking run's measures use it, so its review is
   * measured again when the cut changes. Resolves to that job, or 'none' when there is nothing to follow.
   */
  abstract save(id: string, choice: FaintChoice): Promise<Job>;

  /**
   * Submits the recording's cut-off: kept (on, at the offset), and written as detector labels: the tracks it leaves out
   * are not targets, the ones it keeps are. Rejects when the recording has no review. Resolves to the cut-off as kept.
   */
  abstract submit(id: string, offset: number): Promise<FaintSetting>;

  /**
   * The recordings to set a cut-off in, in the area queue's order (added recordings first, then the most recent of each
   * scenario), leaving out probes, other games, skipped ones and those with a submitted cut-off.
   */
  abstract queue(): Promise<string[]>;

  /** Leaves the recording out of the queue from now on. */
  abstract skip(id: string): Promise<void>;

  /** The labels kept here, to download; null where they are written where training reads them (the server, the app). */
  abstract readonly labels: CutoffLabelsStore | null;
}
