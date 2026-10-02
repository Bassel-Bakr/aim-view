import { ResourceRef } from '@angular/core';
import { StatsChange, StatsChoice, StatsPairing } from '../api';

/**
 * A recording's stats file: the one it has, the ones to pair it with, and the user's choice. Each mode provides one
 * (modes/mode.*.ts).
 */
export abstract class StatsFiles {
  /** It can list KovaaK's stats files to pair with (it reaches the stats folder). */
  abstract readonly searches: boolean;

  /**
   * The open recording's stats file and the files to pair it with: its scenario's, or with a query those of every
   * scenario whose name holds it. Call it where a resource can be made (a field of a component or service).
   */
  abstract pairing(
    id: () => string | undefined,
    query: () => string | null,
  ): ResourceRef<StatsPairing | undefined>;

  /** Pairs the recording with one of the listed files, or with none; or finds it by name and time again. */
  abstract choose(id: string, choice: StatsChoice): Promise<StatsChange>;

  /** Pairs the recording with a stats file from this computer. Rejects a file that is not one of KovaaK's. */
  abstract pairFile(id: string, file: File): Promise<StatsChange>;
}
