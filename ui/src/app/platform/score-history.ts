/**
 * The ScoreHistory contract: every past run of a scenario, from KovaaK's stats files. In: each
 * mode's implementation (modes/mode.*.ts). Out: the run page's progress chart.
 */

import { ResourceRef } from '@angular/core';

/**
 * A past run of a scenario, from its stats file: when it ended (the file name's time stamp, yyyy.mm.dd-hh.mm.ss), its
 * score, its kills, and its accuracy (hits over shots). Kills and accuracy are null when the file lacks them.
 */
export interface PastRun {
  /** When the run ended (yyyy.mm.dd-hh.mm.ss, from the file's name). */
  stamp: string;
  /** The run's score. */
  score: number;
  /** How many kills it had; null when the file lacks them. */
  kills: number | null;
  /** Hits over shots, 0 to 1; null when the file lacks them. */
  accuracy: number | null;
}

/**
 * Every past run of a scenario, from KovaaK's stats files (the files with a score). Each mode provides one
 * (modes/mode.*.ts).
 */
export abstract class ScoreHistory {
  /**
   * The scenario's runs, oldest first; empty when the mode cannot reach the stats files yet. Call it where a resource
   * can be made (a field of a component or service).
   */
  abstract runs(scenario: () => string | undefined): ResourceRef<PastRun[] | undefined>;
}
