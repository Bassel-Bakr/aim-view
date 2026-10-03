/** Where a label's crop lies: its frame (counted from the video's first frame at time 0) and its corner (pixels). */
export interface CropPlace {
  frame: number;
  x0: number;
  y0: number;
}

/** What the cut-off worker is asked: the recording's file, where the core is, and the crops to read. */
export interface CutoffWork {
  file: Blob;
  coreUrl: string;
  crops: CropPlace[];
}

/** A crop's pixels, as hand_crops.py reads them: the frame's RGB at 1280 x 720 and the fixed map, 256 x 256 each. */
export interface CropPixels {
  rgb: Uint8Array<ArrayBuffer>;
  fixed: Uint8Array<ArrayBuffer>;
}

/** Every crop's pixels, in the order asked. */
export interface CutoffRead {
  kind: 'read';
  crops: CropPixels[];
}

export interface CutoffFailed {
  kind: 'error';
  error: string;
}

/** What the cut-off worker says back. */
export type CutoffReply = CutoffRead | CutoffFailed;
