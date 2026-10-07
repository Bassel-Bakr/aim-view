/**
 * The review core loaded on the page itself (not in a worker). In: a review's run parts from its
 * workers (browser-review.ts), a submitted cut-off's tracks (browser-faint-cutoffs.ts) and a
 * crop's scene (the Crops page). Out: the joined review, the cut-off's label crops and what a
 * crop's shapes show.
 */

import { Service } from '@angular/core';
import { AreaRect, Scene, SceneView, TrackFrame, Tracks } from '../../api';
import { CutoffRow } from '../service/cutoff-labels';
import { Core } from './core';
import { HudReading, RunPart, VideoReadings } from './review-messages';

/**
 * What the labels of a submitted cut-off are made from (src/faint.rs: `CutoffRequest`): the
 * tracks' frames, the recording's name, the run's first and last frames (null: not known, no
 * labels), the excluded areas (null: KovOBS's layout), the cut-off's offset, and how near the
 * crosshair a score is not counted (a tracking run 0, a clicking run 2 degrees).
 */
export interface CutoffRequest {
  /** Every frame of the review's tracks (tracks.json's frames). */
  frames: TrackFrame[];
  /** The recording's name, as the rows' video field holds it. */
  video: string;
  /** The run's first frame; null when it is not known (then there are no labels). */
  start: number | null;
  /** The run's last frame; null when it is not known (then there are no labels). */
  end: number | null;
  /** The excluded areas; null for KovOBS's layout. */
  exclude: AreaRect[] | null;
  /** The cut-off's offset, as the user set it. */
  offset: number;
  /** How near the crosshair a score is not counted, in degrees: 0 tracking, 2 clicking. */
  near: number;
}

/**
 * A label's crop (src/faint.rs: `CutoffCrop`): its frame and corner (pixels at 1280 x 720), the
 * boxes it keeps (crop pixels: center and size, written as float32) and its row in checked.jsonl.
 */
export interface CutoffCrop {
  /** The frame the crop is cut from, its index in the recording. */
  frame: number;
  /** The crop's left edge, in pixels at 1280 x 720. */
  x0: number;
  /** The crop's top edge, in pixels at 1280 x 720. */
  y0: number;
  /** The boxes it keeps, as the .npz holds them (crop pixels: center and size). */
  boxes: number[][];
  /** Its row in checked.jsonl. */
  row: CutoffRow;
}

/**
 * What a crop's shapes show (src/shapes.rs `visible`) is asked for: the scene, on a crop of width
 * x height.
 */
export interface ShapesRequest {
  /** The targets drawn on the crop, with their shapes and roles. */
  scene: Scene;
  /** The crop's width in pixels. */
  width: number;
  /** The crop's height in pixels. */
  height: number;
}

/** Why the core made nothing. */
interface CoreRefusal {
  /** The core's reason, in words. */
  error: string;
}

/**
 * A review's runs joined (src/session.rs: `Joined`): the tracks (tracks.json), the video's
 * readings and what the HUD read (null: nothing).
 */
export interface JoinedReview {
  /** The tracks of the whole review, as tracks.json keeps them. */
  tracks: Tracks;
  /** The camera's readings and the countdown, for the whole video. */
  readings: VideoReadings;
  /** What the HUD read; null when the recording shows no HUD the core reads. */
  hud: HudReading | null;
}

/** A core export that takes text and hands text back. */
type TextCall = (core: Core, ptr: number, len: number) => number;

/**
 * The review core on the page itself, for what the page works out besides the review service: a
 * review's runs joined (the tracking runs in workers), a cut-off's crops, and what a crop's shapes
 * show (the Crops page, in every mode).
 */
@Service()
export class CoreModule {
  /** The core, once asked for; loaded once for the page. */
  private core: Promise<Core> | null = null;

  /** Loads the core from core/aimview.wasm beside the page, the first time it is asked for. */
  private load(): Promise<Core> {
    this.core ??= Core.load(new URL('core/aimview.wasm', document.baseURI).href);
    return this.core;
  }

  /** Calls a core export with a text, and gives its text answer (freed in the core). */
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
   * A review's runs joined in order (src/session.rs: `Joining`): the parts each review worker
   * gave, the first's setup and fixed map (every run's are the same). `detector`: the detector that
   * ran, as tracks.json names it. Rejects with the core's error.
   */
  async joinReview(parts: RunPart[], detector: string): Promise<JoinedReview> {
    const core = await this.load();
    const review = core.review(parts[0].setup);
    const fixed = core.reserve(parts[0].fixed.length);
    core.bytes(fixed).set(parts[0].fixed);
    const joining = core.exports.review_joining(review, fixed.ptr);
    core.free(fixed);
    core.exports.review_free(review);
    for (const part of parts) {
      core.textIn(part.track, (track, trackLen) =>
        core.textIn(part.watch, (watch, watchLen) =>
          core.exports.joining_add(joining, track, trackLen, watch, watchLen),
        ),
      );
    }
    const joined = core.textIn(detector, (ptr, len) =>
      core.exports.joining_finish(joining, ptr, len),
    );
    return JSON.parse(core.takeOutcome(joined)) as JoinedReview;
  }

  /**
   * What a crop's shapes show (src/shapes.rs `visible`): each target's visible pixels and box, as
   * the training labels get them (aimview-tool crop-labels uses the same code). Rejects a scene the
   * core refuses.
   */
  async shapesVisible(request: ShapesRequest): Promise<SceneView> {
    const text = await this.call(
      (core, ptr, len) => core.exports.shapes_visible(ptr, len),
      JSON.stringify(request),
    );
    const out = JSON.parse(text) as SceneView | CoreRefusal;
    if ('error' in out) throw new Error(out.error);
    return out;
  }

  /**
   * The crops a submitted cut-off's labels take, as Python's hand_crops.py picks them
   * (src/faint.rs). Rejects a request the core refuses.
   */
  async cutoffCrops(request: CutoffRequest): Promise<CutoffCrop[]> {
    const text = await this.call(
      (core, ptr, len) => core.exports.cutoff_crops(ptr, len),
      JSON.stringify(request),
    );
    const out = JSON.parse(text) as CutoffCrop[] | CoreRefusal;
    if ('error' in out) throw new Error(out.error);
    return out;
  }
}
