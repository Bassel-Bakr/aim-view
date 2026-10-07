/**
 * The messages between the page and the cut-off's worker (cutoff.worker.ts). In: the crops a
 * submit's labels need (browser-faint-cutoffs.ts). Out: each crop's pixels, or why they could not
 * be read.
 */

/**
 * Where a label's crop lies: its frame (counted from the video's first frame at time 0) and its
 * corner (pixels).
 */
export interface CropPlace {
  /** The frame's index, counted from the first frame at time 0. */
  frame: number;
  /** The crop's left edge, in pixels of the 1280 x 720 frame. */
  x0: number;
  /** The crop's top edge, in pixels of the 1280 x 720 frame. */
  y0: number;
}

/**
 * What the cut-off worker is asked: the recording's file, where the core is, and the crops to
 * read.
 */
export interface CutoffWork {
  /** The recording's video file. */
  file: Blob;
  /** The address of the core's WebAssembly, which the worker loads. */
  coreUrl: string;
  /** The crops to read, in the order the answer gives them. */
  crops: CropPlace[];
}

/**
 * A crop's pixels, as hand_crops.py reads them: the frame's RGB at 1280 x 720 and the fixed map,
 * 256 x 256 each.
 */
export interface CropPixels {
  /** The crop of the frame's RGB, 3 bytes a pixel, row by row. */
  rgb: Uint8Array<ArrayBuffer>;
  /** The same crop of the fixed map, a byte a pixel. */
  fixed: Uint8Array<ArrayBuffer>;
}

/** Every crop's pixels, in the order asked. */
export interface CutoffRead {
  /** Tells this answer from an error. */
  kind: 'read';
  /** One for each crop asked, in its order. */
  crops: CropPixels[];
}

/** The cut-off worker failed: why, in words. */
export interface CutoffFailed {
  /** Tells this answer from the pixels read. */
  kind: 'error';
  /** The error's message. */
  error: string;
}

/** What the cut-off worker says back. */
export type CutoffReply = CutoffRead | CutoffFailed;
