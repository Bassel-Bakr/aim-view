import { VideoSample } from 'mediabunny';
import { Core, CoreBlock, matrixNumber } from './core';
import { FrameFormat } from './review-messages';

/**
 * A frame's planes written into the core's memory as YUV 4:2:0 (Y, U, V), whatever layout the decoder gave. An I420
 * frame (the software decoder's) is copied by the decoder straight into place, packed; another, through scratch.
 */
async function writeI420(
  sample: VideoSample,
  core: Core,
  block: CoreBlock,
  scratch: Uint8Array,
): Promise<void> {
  const { width: w, height: h } = sample.visibleRect;
  if (sample.format === 'I420') {
    const [cw, ch] = [w >> 1, h >> 1];
    const packed: PlaneLayout[] = [
      { offset: 0, stride: w },
      { offset: w * h, stride: cw },
      { offset: w * h + cw * ch, stride: cw },
    ];
    // the core's memory can grow while the copy waits (the tracker takes a call's maps meanwhile): copy again then
    for (;;) {
      const memory = core.x.memory.buffer;
      try {
        await sample.copyTo(core.bytes(block), { layout: packed, rect: sample.visibleRect });
      } catch (e) {
        if (core.x.memory.buffer === memory) throw e;
      }
      if (core.x.memory.buffer === memory) return;
    }
  }
  if (sample.format !== 'NV12') {
    throw new Error(
      `The decoder gave ${sample.format ?? 'an unknown'} frames; the review reads 8-bit YUV 4:2:0`,
    );
  }
  // NV12 (a hardware decoder's): the luma as it is, the chroma's interleaved U and V apart
  const [yp, up] = await sample.copyTo(scratch);
  const out = core.bytes(block);
  for (let r = 0; r < h; r++)
    out.set(scratch.subarray(yp.offset + r * yp.stride, yp.offset + r * yp.stride + w), r * w);
  const cw = w >> 1;
  const ch = h >> 1;
  const u0 = w * h;
  const v0 = u0 + cw * ch;
  for (let r = 0; r < ch; r++) {
    const row = up.offset + r * up.stride;
    for (let c = 0; c < cw; c++) {
      out[u0 + r * cw + c] = scratch[row + 2 * c];
      out[v0 + r * cw + c] = scratch[row + 2 * c + 1];
    }
  }
}

/**
 * Decoded frames into a worker's core, which turns them into ffmpeg's pixels: its converter (src/wasm.rs:
 * converter_new), made for the first frame's size and colors, and the block each frame is written into.
 */
export class FrameConverter {
  /** The converter; 0 until the first frame. */
  converter = 0;
  /** The frames' format, from the first frame; null until then. */
  format: FrameFormat | null = null;
  private block: CoreBlock | null = null;
  private scratch = new Uint8Array(0);

  constructor(private readonly core: Core) {}

  /**
   * A decoded frame written into the core's memory as YUV 4:2:0 at its own size, and closed: the block it is in, the
   * same for every frame (the next frame takes its place).
   */
  async write(s: VideoSample): Promise<CoreBlock> {
    const { width: w, height: h } = s.visibleRect;
    if (!this.format) {
      this.format = {
        width: w,
        height: h,
        matrix: matrixNumber(s.colorSpace.matrix),
        full: s.colorSpace.fullRange ? 1 : 0,
      };
      this.converter = this.core.x.converter_new(w, h, this.format.matrix, this.format.full);
      this.block = this.core.reserve((w * h * 3) / 2);
    }
    if (this.scratch.length < s.allocationSize()) this.scratch = new Uint8Array(s.allocationSize());
    const block = this.block as CoreBlock;
    await writeI420(s, this.core, block, this.scratch);
    s.close();
    return block;
  }

  /** The frame in `block` at 1280 x 720 as YUV 4:2:0, into `out` (ffmpeg's `scale=1280:720:flags=area`). */
  yuv720(block: CoreBlock, out: CoreBlock): void {
    this.core.x.converter_yuv420p(this.converter, block.ptr, block.len, out.ptr);
  }

  /** The frame in `block` at 1280 x 720 as RGB, into `out`. */
  rgb(block: CoreBlock, out: CoreBlock): void {
    this.core.x.converter_rgb24(this.converter, block.ptr, block.len, out.ptr);
  }

  /** The converter is done. */
  free(): void {
    if (this.converter) this.core.x.converter_free(this.converter);
    this.converter = 0;
  }
}
