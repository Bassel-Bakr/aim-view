/** The review core as WebAssembly (src/wasm.rs): its exports, and the copying in and out of its memory. */

import { AreaBox } from '../../api';

/** The kind of the challenge's end screen (src/popup.rs: END_SCREEN): excluded only while it shows. */
export const END_SCREEN = 'challenge_results';

/** The core module's exports. Pointers and sizes are byte offsets and counts in its memory. */
export interface CoreExports {
  memory: WebAssembly.Memory;
  alloc(len: number): number;
  dealloc(ptr: number, len: number): void;
  tracker_new(areas: number, count: number, cap: number): number;
  tracker_new_ends(areas: number, ends: number, count: number, cap: number): number;
  tracker_new_kovobs(cap: number): number;
  tracker_set_model(tracker: number, text: number, len: number): number;
  tracker_watch(tracker: number, rgb: number): void;
  tracker_push_maps(tracker: number, score: number, reg: number, gw: number, gh: number): number;
  tracker_finish(tracker: number): number;
  tracker_start_at(tracker: number, first: number): void;
  tracker_part(tracker: number): number;
  tracker_add_part(tracker: number, part: number, len: number): number;
  converter_new(w: number, h: number, matrix: number, full: number): number;
  converter_rgb24(converter: number, yuv: number, len: number, out: number): void;
  converter_yuv420p(converter: number, yuv: number, len: number, out: number): void;
  converter_luma(converter: number, y: number, len: number, out: number): void;
  camera_rgb_rows(): number;
  review_version(): number;
  converter_free(converter: number): void;
  fixed_new(): number;
  fixed_add(fixed: number, yuv: number): void;
  fixed_finish(fixed: number, out: number): void;
  scenario_facts(text: number, len: number): number;
  review_report(request: number, len: number): number;
  cutoff_crops(request: number, len: number): number;
  camera_new(fixed: number): number;
  camera_new_areas(areas: number, count: number, fixed: number): number;
  camera_add(camera: number, yuv: number, rgb: number): void;
  camera_finish(camera: number, frames: number, len: number): number;
  camera_part(camera: number): number;
  camera_skip(camera: number, frames: number): void;
  camera_add_part(camera: number, part: number, len: number): number;
  hud_new(w: number, h: number, full: number): number;
  hud_add_key(hud: number, y: number, len: number): void;
  hud_add(hud: number, y: number, len: number): void;
  hud_skip(hud: number, frames: number): void;
  hud_part(hud: number): number;
  hud_add_part(hud: number, part: number, len: number): number;
  hud_finish(hud: number): number;
  mouse_read(log: number, logLen: number, request: number, len: number): number;
  areas_new(): number;
  areas_add(finder: number, yuv: number): void;
  areas_finish(finder: number, session: number, len: number): number;
  hud_session_box(hud: number): number;
  areas_sample(input: number, len: number): number;
  areas_find(input: number, len: number): number;
  areas_learn(input: number, len: number): number;
}

/** A block of the core's memory, reserved until freed. */
export interface CoreBlock {
  ptr: number;
  len: number;
}

/** The colour matrices the converter knows, by the number it takes (src/wasm.rs: `converter_new`). */
const MATRICES: Record<string, number> = {
  bt709: 0,
  bt470bg: 1,
  smpte170m: 1,
  fcc: 2,
  smpte240m: 3,
  'bt2020-ncl': 4,
};

/** The converter's number for a frame's colour matrix: BT.601 when the frame does not say, as ffmpeg takes it. */
export function matrixNumber(matrix: string | null | undefined): number {
  return MATRICES[matrix ?? ''] ?? 1;
}

export class Core {
  private constructor(readonly x: CoreExports) {}

  static async load(url: string): Promise<Core> {
    const { instance } = await WebAssembly.instantiateStreaming(fetch(url));
    return new Core(instance.exports as unknown as CoreExports);
  }

  reserve(len: number): CoreBlock {
    return { ptr: this.x.alloc(len), len };
  }

  free(block: CoreBlock): void {
    this.x.dealloc(block.ptr, block.len);
  }

  /** The block's bytes. A fresh view each time: the memory can grow, which leaves older views empty. */
  bytes(block: CoreBlock): Uint8Array {
    return new Uint8Array(this.x.memory.buffer, block.ptr, block.len);
  }

  floats(block: CoreBlock): Float32Array {
    return new Float32Array(this.x.memory.buffer, block.ptr, block.len / 4);
  }

  /**
   * A tracker that ignores these areas (shares of the frame; the challenge's end screen only while it shows); cap: the
   * scenario's target count, 0 for none.
   */
  tracker(areas: readonly AreaBox[], cap: number): number {
    const ends = this.reserve(areas.length);
    this.bytes(ends).set(areas.map((a) => (a[4] === END_SCREEN ? 1 : 0)));
    const tracker = this.withAreas(areas, (ptr, count) =>
      this.x.tracker_new_ends(ptr, ends.ptr, count, cap),
    );
    this.free(ends);
    return tracker;
  }

  /**
   * The detector model's settings file (detector_<name>.json: python/model/MODEL_FILE.md) for a tracker, before its
   * first frame. Throws when the core cannot read it.
   */
  setModel(tracker: number, settings: string): void {
    const why = this.takeText(
      this.textIn(settings, (ptr, len) => this.x.tracker_set_model(tracker, ptr, len)),
    );
    if (why) throw new Error(`The model's settings file: ${why}`);
  }

  /**
   * A camera watch whose tiles keep clear of these areas (KovOBS's layout when there are none) and of the fixed map
   * (1280 x 720, at `fixed` in the core's memory).
   */
  camera(areas: readonly AreaBox[], fixed: number): number {
    return this.withAreas(areas, (ptr, count) => this.x.camera_new_areas(ptr, count, fixed));
  }

  /** A text in the core's memory (UTF-8) for one call: its place and length. */
  textIn(text: string, use: (ptr: number, len: number) => number): number {
    const bytes = new TextEncoder().encode(text);
    const block = this.reserve(bytes.length);
    this.bytes(block).set(bytes);
    const out = use(block.ptr, bytes.length);
    this.free(block);
    return out;
  }

  /** The areas as the core takes them (4 f64s each), for one call. */
  private withAreas(
    areas: readonly AreaBox[],
    use: (ptr: number, count: number) => number,
  ): number {
    const block = this.reserve(areas.length * 4 * 8);
    const bounds = new Float64Array(this.x.memory.buffer, block.ptr, areas.length * 4);
    areas.forEach(([x0, y0, x1, y1], i) => bounds.set([x0, y0, x1, y1], i * 4));
    const made = use(block.ptr, areas.length);
    this.free(block);
    return made;
  }

  /** A result the core hands back: its length (u32), then its bytes, read as text and freed. */
  takeText(ptr: number): string {
    const len = new DataView(this.x.memory.buffer).getUint32(ptr, true);
    const text = new TextDecoder().decode(new Uint8Array(this.x.memory.buffer, ptr + 4, len));
    this.x.dealloc(ptr, 4 + len);
    return text;
  }
}
