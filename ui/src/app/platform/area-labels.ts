/**
 * The AreaLabels contract: a recording's excluded areas, their kinds, and the area finder. In:
 * each mode's implementation (modes/mode.*.ts). Out: the run page's Excluded areas editor
 * (run/areas/).
 */

import { ResourceRef } from '@angular/core';
import {
  AreaBox,
  AreaKind,
  AreaSet,
  FoundAreas,
  KeptAreas,
  KindEdit,
  RecordingAreas,
} from '../api';

/**
 * A recording's excluded areas (the parts of the screen the review ignores: a webcam, an overlay), the kinds an area
 * can be, and the area finder, which proposes areas and learns from the ones the user saves. Each mode provides one
 * (modes/mode.*.ts).
 */
export abstract class AreaLabels {
  /** Why this mode cannot find areas yet, in words; null when it can. */
  abstract readonly finderMissing: string | null;

  /**
   * The recording's areas: the ones saved for it, else for an added recording the ones last saved for one, else
   * KovOBS's layout; with every kind. Call it where a resource can be made (a field of a component or service).
   */
  abstract areas(id: () => string | undefined): ResourceRef<RecordingAreas | undefined>;

  /** KovOBS's layout: the areas excluded by default. */
  abstract layout(): Promise<AreaSet>;

  /**
   * The finder's proposal: with copy, the user's own areas from a recording with the same layout when there is one;
   * else the areas found in this one, named from what the user taught or by rules. Rejects where there is no finder.
   */
  abstract find(id: string, copy: boolean): Promise<FoundAreas>;

  /**
   * Keeps the recording's areas, and the finder learns from them. Resolves to the areas as kept, with the review it
   * started where the mode tracks again by itself (else the page decides whether to).
   */
  abstract save(id: string, boxes: AreaBox[]): Promise<KeptAreas>;

  /** Adds a kind (id null), or gives a kind a new name and description. Resolves to every kind. */
  abstract saveKind(kind: KindEdit): Promise<AreaKind[]>;
}
