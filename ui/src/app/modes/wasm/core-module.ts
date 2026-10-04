import { Injectable } from '@angular/core';
import { AreaRect, TrackFrame, Tracks } from '../../api';
import { CutoffRow } from '../web-files/cutoff-labels';
import { Core } from './core';
import { HudReading, RunPart, VideoReadings } from './review-messages';

/**
 * What the labels of a submitted cut-off are made from (src/faint.rs: `CutoffRequest`): the tracks' frames, the
 * recording's name, the run's first and last frames (null: not known, no labels), the excluded areas (null: KovOBS's
 * layout), the cut-off's offset, and how near the crosshair a score is not counted (a tracking run 0, a clicking run 2
 * degrees).
 */
export interface CutoffRequest {
  frames: TrackFrame[];
  video: string;
  start: number | null;
  end: number | null;
  exclude: AreaRect[] | null;
  offset: number;
  near: number;
}

/**
 * A label's crop (src/faint.rs: `CutoffCrop`): its frame and corner (pixels at 1280 x 720), the boxes it keeps (crop
 * pixels: center and size, written as float32) and its row in checked.jsonl.
 */
export interface CutoffCrop {
  frame: number;
  x0: number;
  y0: number;
  boxes: number[][];
  row: CutoffRow;
}

/** Why the core made nothing. */
interface CoreRefusal {
  error: string;
}

/**
 * A review's runs joined (src/session.rs: `Joined`): the tracks (tracks.json), the video's readings and what the HUD
 * read (null: nothing).
 */
export interface JoinedReview {
  tracks: Tracks;
  readings: VideoReadings;
  hud: HudReading | null;
}

/** A core export that takes text and hands text back. */
type TextCall = (core: Core, ptr: number, len: number) => number;

/**
 * The review core on the page itself, for what the page works out besides the review service: a review's runs joined
 * (the tracking runs in workers) and a cut-off's crops.
 */
@Injectable({ providedIn: 'root' })
export class CoreModule {
  private core: Promise<Core> | null = null;

  private load(): Promise<Core> {
    this.core ??= Core.load(new URL('core/aimview.wasm', document.baseURI).href);
    return this.core;
  }

  private async call(fn: TextCall, text: string): Promise<string> {
    const core = await this.load();
    const bytes = new TextEncoder().encode(text);
    const block = core.reserve(bytes.length);
    core.bytes(block).set(bytes);
    const out = fn(core, block.ptr, bytes.length);
    core.free(block);
    return core.takeText(out);
  }

  /**
   * A review's runs joined in order (src/session.rs: `Joining`): the parts each review worker gave, the first's setup
   * and fixed map (every run's are the same). `detector`: the detector that ran, as tracks.json names it.
   */
  async joinReview(parts: RunPart[], detector: string): Promise<JoinedReview> {
    const core = await this.load();
    const review = core.review(parts[0].setup);
    const fixed = core.reserve(parts[0].fixed.length);
    core.bytes(fixed).set(parts[0].fixed);
    const joining = core.x.review_joining(review, fixed.ptr);
    core.free(fixed);
    core.x.review_free(review);
    for (const p of parts) {
      core.textIn(p.track, (track, trackLen) =>
        core.textIn(p.watch, (watch, watchLen) =>
          core.x.joining_add(joining, track, trackLen, watch, watchLen),
        ),
      );
    }
    const joined = core.textIn(detector, (ptr, len) => core.x.joining_finish(joining, ptr, len));
    return JSON.parse(core.takeOutcome(joined)) as JoinedReview;
  }

  /** The crops a submitted cut-off's labels take, as Python's hand_crops.py picks them: src/faint.rs. */
  async cutoffCrops(request: CutoffRequest): Promise<CutoffCrop[]> {
    const text = await this.call((c, p, n) => c.x.cutoff_crops(p, n), JSON.stringify(request));
    const out = JSON.parse(text) as CutoffCrop[] | CoreRefusal;
    if ('error' in out) throw new Error(out.error);
    return out;
  }
}
