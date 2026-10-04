/** What the area finder's worker is asked: the recording's file and where the core is. */
export interface FinderWork {
  file: Blob;
  coreUrl: string;
}

/** What the area finder found in the recording: its JSON (src/areas.rs: Found). */
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
