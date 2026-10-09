/**
 * The review core as WebAssembly (src/wasm.rs): its exports, and the copying in and out of its
 * memory. In: the core's .wasm file (`bun run assets` builds it into ui/generated/). Out: a `Core`
 * that the workers (review, camera, area finder, cut-off) and core-module.ts call.
 */

/**
 * The core module's exports (src/wasm.rs has each one's full doc). Pointers and sizes are byte
 * offsets and counts in its memory. A handle (a tracker, a review) is a pointer the core made;
 * a function that ends in finish or part frees its handle. A text result is a pointer to its
 * length (u32), then its UTF-8 bytes: read it with `Core.takeText`.
 */
export interface CoreExports {
  /** The core's memory, which every pointer is an offset into; it can grow. */
  memory: WebAssembly.Memory;
  /** Reserves len bytes (8-byte aligned); gives their offset. */
  alloc(len: number): number;
  /** Frees what one alloc call reserved. */
  dealloc(ptr: number, len: number): void;
  /** A converter for w x h YUV 4:2:0 frames; matrix from `matrixNumber`, full 1 for pc range. */
  converter_new(w: number, h: number, matrix: number, full: number): number;
  /** One frame as RGB at 1280 x 720 into out (1280 * 720 * 3 bytes). */
  converter_rgb24(converter: number, yuv: number, len: number, out: number): void;
  /** One frame as YUV 4:2:0 at 1280 x 720 into out (1280 * 720 * 3 / 2 bytes). */
  converter_yuv420p(converter: number, yuv: number, len: number, out: number): void;
  /** The 720p RGB rows the countdown test reads, as from + (to << 16). */
  camera_rgb_rows(): number;
  /** Frees a converter. */
  converter_free(converter: number): void;
  /** A fixed map builder, fed the key frames. */
  fixed_new(): number;
  /** One key frame, YUV 4:2:0 at 1280 x 720. */
  fixed_add(fixed: number, yuv: number): void;
  /** The map into out (1280 * 720 bytes, 1 fixed, 0 not); frees the builder. */
  fixed_finish(fixed: number, out: number): void;
  /** A submitted cut-off's label crops: src/faint.rs `CutoffRequest` in, crops or {error} out. */
  cutoff_crops(request: number, len: number): number;
  /** What a crop's shapes show: {scene, width, height} in, a `SceneView` or {error} out. */
  shapes_visible(request: number, len: number): number;
  /** A HUD watch (src/hud/) for w x h frames; full 1 when their Y spans 0 to 255. */
  hud_new(w: number, h: number, full: number): number;
  /** One key frame's Y plane, before any frame. */
  hud_add_key(hud: number, y: number, len: number): void;
  /** An area finder (src/areas.rs `AreaFinder`), fed the frames areas_sample picks. */
  areas_new(): number;
  /** One frame, YUV 4:2:0 at 1280 x 720. */
  areas_add(finder: number, yuv: number): void;
  /** The areas found as JSON text (src/areas.rs `Found`), given the session box; frees it. */
  areas_finish(finder: number, session: number, len: number): number;
  /** KovaaK's session box from the key frames, as JSON text (or null), for areas_finish. */
  hud_session_box(hud: number): number;
  /** The frames the finder reads: {keys, times, duration} in; null (key frames) or indexes out. */
  areas_sample(input: number, len: number): number;
  /** A review from its setup (src/session.rs `Setup`); 0 when unreadable or with no frames. */
  review_new(setup: number, len: number): number;
  /** The model's settings file for the review; gives a text: empty when read, else why not. */
  review_set_model(review: number, text: number, len: number): number;
  /** The review's runs as JSON text (src/session.rs `Run` each). */
  review_runs(review: number): number;
  /** Frees the review; what it made (trackings, watches, joins) lives on. */
  review_free(review: number): void;
  /** The review's key frame pass, fed each key frame. */
  review_keys(review: number): number;
  /** One key frame: its 720p YUV 4:2:0 (small) and its Y plane as decoded (y, len bytes). */
  keys_add(keys: number, small: number, y: number, len: number): void;
  /** The fixed map into fixed, and the HUD's boxes as JSON text; frees the pass. */
  keys_finish(keys: number, fixed: number): number;
  /** The tracking of one run of the review (src/session.rs `RunTracking`). */
  review_tracking(review: number, run: number): number;
  /** What the next decoded frame is for: a `NEXT_FRAME` value. */
  tracking_next(tracking: number): number;
  /** A tracked frame as RGB at 1280 x 720: its excluded areas are watched for pop-ups. */
  tracking_watch(tracking: number, rgb: number): void;
  /** The detector's maps for the next tracked frame: score (gh x gw) and reg (4 x gh x gw), f32. */
  tracking_maps(tracking: number, score: number, reg: number, gw: number, gh: number): void;
  /** The run's part of the tracking as JSON text (`TrackPart`) or {error}; frees the tracking. */
  tracking_part(tracking: number): number;
  /** One run's watches, from the fixed map and the HUD's boxes (keys_finish's JSON); 0 on error. */
  review_watching(review: number, run: number, fixed: number, hud: number, len: number): number;
  /** One frame the run reads: its decoded Y plane, then its countdown rows of 720p RGB. */
  watching_frame(watching: number, frame: number, len: number): void;
  /** The run's part of the watches as JSON text (`WatchPart`) or {error}; frees the watches. */
  watching_part(watching: number): number;
  /** A join of the runs' parts, in order, from the key frames' fixed map (1280 * 720 bytes). */
  review_joining(review: number, fixed: number): number;
  /** The next run's parts (tracking_part's, watching_part's JSON); 1 when both read, else 0. */
  joining_add(
    joining: number,
    track: number,
    trackLen: number,
    watch: number,
    watchLen: number,
  ): number;
  /** The joined review as JSON text ({tracks, readings, hud}) or {error}; frees the join. */
  joining_finish(joining: number, detector: number, len: number): number;
}

/**
 * The review's frame width: every frame is scaled to 1280 x 720 (ffmpeg's
 * `scale=1280:720:flags=area`).
 */
export const FRAME_WIDTH_PX = 1280;
/** The review's frame height in pixels. */
export const FRAME_HEIGHT_PX = 720;
/** The pixels in a review frame. */
export const FRAME_PIXELS = FRAME_WIDTH_PX * FRAME_HEIGHT_PX;
/** The bytes of an RGB pixel. */
export const RGB_CHANNELS = 3;
/** A review frame's bytes as RGB. */
export const FRAME_RGB_BYTES = FRAME_PIXELS * RGB_CHANNELS;
/** A review frame's bytes as YUV 4:2:0: a byte a pixel, then a quarter as many for U and V each. */
export const FRAME_YUV420_BYTES = (FRAME_PIXELS * 3) / 2;

/** A float's bytes, for a Float32Array view of the core's memory. */
const FLOAT_BYTES = 4;
/** The bytes of the u32 length in front of a result the core hands back. */
const LENGTH_BYTES = 4;
/** BT.601's number: a frame that names no matrix is taken as BT.601, as ffmpeg takes it. */
const BT601 = 1;

/** What `tracking_next` says a decoded frame is for (src/session.rs: `NextFrame`). */
export const NEXT_FRAME = { stop: 0, watch: 1, track: 2 } as const;

/** A core call that hands back JSON or {error}: why it refused. */
interface CoreError {
  /** Why the core refused, in words. */
  error: string;
}

/** A block of the core's memory, reserved until freed. */
export interface CoreBlock {
  /** Its byte offset in the core's memory. */
  ptr: number;
  /** Its size in bytes. */
  len: number;
}

/**
 * The color matrices the converter knows, by the number it takes (src/wasm.rs: `converter_new`),
 * keyed by the names a VideoSample's colorSpace gives.
 */
const MATRICES: Record<string, number> = {
  bt709: 0,
  bt470bg: 1,
  smpte170m: 1,
  fcc: 2,
  smpte240m: 3,
  'bt2020-ncl': 4,
};

/**
 * The converter's number for a frame's color matrix: BT.601 when the frame does not say (or names
 * one it does not know), as ffmpeg takes it.
 */
export function matrixNumber(matrix: string | null | undefined): number {
  return MATRICES[matrix ?? ''] ?? BT601;
}

/** One loaded copy of the core: its exports, and helpers that move bytes and text in and out. */
export class Core {
  /** Keeps the instance's exports; `load` makes a Core. */
  private constructor(readonly exports: CoreExports) {}

  /** Fetches and compiles the core's .wasm at url. Rejects when it cannot be loaded. */
  static async load(url: string): Promise<Core> {
    const { instance } = await WebAssembly.instantiateStreaming(fetch(url));
    return new Core(instance.exports as unknown as CoreExports);
  }

  /** Reserves len bytes of the core's memory; free them with `free`. */
  reserve(len: number): CoreBlock {
    return { ptr: this.exports.alloc(len), len };
  }

  /** Frees a block `reserve` gave. */
  free(block: CoreBlock): void {
    this.exports.dealloc(block.ptr, block.len);
  }

  /**
   * The block's bytes. A fresh view each time: the memory can grow, which leaves older views
   * empty.
   */
  bytes(block: CoreBlock): Uint8Array {
    return new Uint8Array(this.exports.memory.buffer, block.ptr, block.len);
  }

  /** The block as 32-bit floats, a fresh view each time (as `bytes`). */
  floats(block: CoreBlock): Float32Array {
    return new Float32Array(this.exports.memory.buffer, block.ptr, block.len / FLOAT_BYTES);
  }

  /**
   * A review from its setup (src/session.rs: `Setup`, as JSON). Throws when the core cannot read
   * it.
   */
  review(setup: string): number {
    const review = this.textIn(setup, (ptr, len) => this.exports.review_new(ptr, len));
    if (!review) throw new Error("The review's setup could not be read");
    return review;
  }

  /**
   * The detector model's settings file (detector_<name>.json: python/model/MODEL_FILE.md) for a
   * review, before its runs start. Throws when the core cannot read it.
   */
  setModel(review: number, settings: string): void {
    const why = this.takeText(
      this.textIn(settings, (ptr, len) => this.exports.review_set_model(review, ptr, len)),
    );
    if (why) throw new Error(`The model's settings file: ${why}`);
  }

  /**
   * A result the core hands back as JSON or {error}: the JSON, read and freed. Throws the core's
   * error.
   */
  takeOutcome(ptr: number): string {
    const text = this.takeText(ptr);
    if (text.startsWith('{"error"')) throw new Error((JSON.parse(text) as CoreError).error);
    return text;
  }

  /**
   * A text in the core's memory (UTF-8) for one call: use gets its place and length, and its
   * answer is given back. The text is freed after the call.
   */
  textIn(text: string, use: (ptr: number, len: number) => number): number {
    const bytes = new TextEncoder().encode(text);
    const block = this.reserve(bytes.length);
    this.bytes(block).set(bytes);
    const out = use(block.ptr, bytes.length);
    this.free(block);
    return out;
  }

  /** A result the core hands back: its length (u32), then its bytes, read as text and freed. */
  takeText(ptr: number): string {
    const len = new DataView(this.exports.memory.buffer).getUint32(ptr, true);
    const text = new TextDecoder().decode(
      new Uint8Array(this.exports.memory.buffer, ptr + LENGTH_BYTES, len),
    );
    this.exports.dealloc(ptr, LENGTH_BYTES + len);
    return text;
  }
}
