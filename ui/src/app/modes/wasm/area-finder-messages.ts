/**
 * The messages between the page and the area finder's worker (area-finder.worker.ts). In: the
 * page's request (page-area-finder.ts). Out: the worker's answer, the areas found or why not.
 */

/** What the area finder's worker is asked: the recording's file and where the core is. */
export interface FinderWork {
  /** The recording's video file. */
  file: Blob;
  /** The address of the core's WebAssembly, which the worker loads. */
  coreUrl: string;
}

/** What the area finder found in the recording: its JSON (src/areas.rs: Found). */
export interface FinderFound {
  /** Tells this answer from an error. */
  kind: 'found';
  /** The areas found, as the core's JSON text. */
  found: string;
}

/** The area finder failed: why, in words. */
export interface FinderFailed {
  /** Tells this answer from the areas found. */
  kind: 'error';
  /** The error's message. */
  error: string;
}

/** What the area finder's worker says back. */
export type FinderReply = FinderFound | FinderFailed;
