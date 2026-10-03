/** The review core as WebAssembly (src/wasm.rs): its exports, and the copying in and out of its memory. */

/** The core module's exports. Pointers and sizes are byte offsets and counts in its memory. */
export interface CoreExports {
  memory: WebAssembly.Memory;
  alloc(len: number): number;
  dealloc(ptr: number, len: number): void;
  tracker_new_kovobs(cap: number): number;
  tracker_watch(tracker: number, rgb: number): void;
  tracker_push_maps(tracker: number, score: number, reg: number, gw: number, gh: number): number;
  tracker_finish(tracker: number): number;
  converter_new(w: number, h: number, matrix: number, full: number): number;
  converter_rgb24(converter: number, yuv: number, len: number, out: number): void;
  converter_yuv420p(converter: number, yuv: number, len: number, out: number): void;
  converter_luma(converter: number, y: number, len: number, out: number): void;
  camera_rgb_rows(): number;
  converter_free(converter: number): void;
  fixed_new(): number;
  fixed_add(fixed: number, yuv: number): void;
  fixed_finish(fixed: number, out: number): void;
  scenario_facts(text: number, len: number): number;
  review_report(request: number, len: number): number;
  camera_new(fixed: number): number;
  camera_add(camera: number, yuv: number, rgb: number): void;
  camera_finish(camera: number, frames: number, len: number): number;
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

  /** A result the core hands back: its length (u32), then its bytes, read as text and freed. */
  takeText(ptr: number): string {
    const len = new DataView(this.x.memory.buffer).getUint32(ptr, true);
    const text = new TextDecoder().decode(new Uint8Array(this.x.memory.buffer, ptr + 4, len));
    this.x.dealloc(ptr, 4 + len);
    return text;
  }
}
