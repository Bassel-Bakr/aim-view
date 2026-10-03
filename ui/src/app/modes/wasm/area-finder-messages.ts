import { AreaRect } from '../../api';

/** What the area finder's worker is asked: the recording's file and where the core is. */
export interface FinderWork {
  file: Blob;
  coreUrl: string;
}

/** What the area finder found in the recording: FinderResult's JSON (src/areas.rs: Found). */
export interface FinderFound {
  kind: 'found';
  found: string;
}

export interface FinderFailed {
  kind: 'error';
  error: string;
}

/** What the area finder's worker says back. */
export type FinderReply = FinderFound | FinderFailed;

/** An area the finder found (src/areas.rs: Area): its box (shares of the frame), its features, and the rules' kind. */
export interface FoundArea {
  box: AreaRect;
  feat: number[];
  rule: string;
}

/**
 * What the area finder found in a recording (src/areas.rs: Found): the frames it read, the areas, and the maps they
 * came from (packed, as its JSON gives them), which describe any area the user draws when the finder learns.
 */
export interface FinderResult {
  frames: number;
  areas: FoundArea[];
  maps: unknown;
}
