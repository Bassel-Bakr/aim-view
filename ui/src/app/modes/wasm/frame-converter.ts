import { VideoSample } from 'mediabunny';
import { Core, CoreBlock, matrixNumber } from './core';
import { FrameFormat } from './review-messages';

/** A frame's planes packed as YUV 4:2:0 (I420): Y, then U, then V, each row as wide as its plane. */
export function i420Layout(width: number, height: number): PlaneLayout[] {
  const chromaWidth = width >> 1;
  const chromaHeight = height >> 1;
  return [
    { offset: 0, stride: width },
    { offset: width * height, stride: chromaWidth },
    { offset: width * height + chromaWidth * chromaHeight, stride: chromaWidth },
  ];
}

/** The error for a frame neither I420 nor NV12; `reader` says what reads it. */
export function unreadableFormat(sample: VideoSample, reader: string): Error {
  return new Error(
    `The decoder gave ${sample.format ?? 'an unknown'} frames; ${reader} 8-bit YUV 4:2:0`,
  );
}

/**
 * An NV12 frame (a hardware decoder's), copied into `scratch` as the decoder lays it out (`planes`), packed into `out`
 * as I420: the luma as it is, the chroma's interleaved U and V apart.
 */
export function packNv12(
  scratch: Uint8Array,
  [lumaPlane, chromaPlane]: PlaneLayout[],
  out: Uint8Array,
  width: number,
  height: number,
): void {
  for (let row = 0; row < height; row++) {
    const from = lumaPlane.offset + row * lumaPlane.stride;
    out.set(scratch.subarray(from, from + width), row * width);
  }
  const chromaWidth = width >> 1;
  const chromaHeight = height >> 1;
  const uStart = width * height;
  const vStart = uStart + chromaWidth * chromaHeight;
  for (let chromaRow = 0; chromaRow < chromaHeight; chromaRow++) {
    const row = chromaPlane.offset + chromaRow * chromaPlane.stride;
    for (let column = 0; column < chromaWidth; column++) {
      out[uStart + chromaRow * chromaWidth + column] = scratch[row + 2 * column];
      out[vStart + chromaRow * chromaWidth + column] = scratch[row + 2 * column + 1];
    }
  }
}

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
  const { width, height } = sample.visibleRect;
  if (sample.format === 'I420') {
    const packed = i420Layout(width, height);
    // the core's memory can grow while the copy waits (the tracker takes a call's maps meanwhile): copy again then
    for (;;) {
      const memory = core.exports.memory.buffer;
      try {
        await sample.copyTo(core.bytes(block), { layout: packed, rect: sample.visibleRect });
      } catch (error) {
        if (core.exports.memory.buffer === memory) throw error;
      }
      if (core.exports.memory.buffer === memory) return;
    }
  }
  if (sample.format !== 'NV12') throw unreadableFormat(sample, 'the review reads');
  const planes = await sample.copyTo(scratch);
  packNv12(scratch, planes, core.bytes(block), width, height);
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
  async write(sample: VideoSample): Promise<CoreBlock> {
    const { width, height } = sample.visibleRect;
    if (!this.format) {
      this.format = {
        width,
        height,
        matrix: matrixNumber(sample.colorSpace.matrix),
        full: !!sample.colorSpace.fullRange,
      };
      this.converter = this.core.exports.converter_new(
        width,
        height,
        this.format.matrix,
        Number(this.format.full),
      );
      this.block = this.core.reserve((width * height * 3) / 2);
    }
    if (this.scratch.length < sample.allocationSize())
      this.scratch = new Uint8Array(sample.allocationSize());
    const block = this.block as CoreBlock;
    await writeI420(sample, this.core, block, this.scratch);
    sample.close();
    return block;
  }

  /** The frame in `block` at 1280 x 720 as YUV 4:2:0, into `out` (ffmpeg's `scale=1280:720:flags=area`). */
  yuv720(block: CoreBlock, out: CoreBlock): void {
    this.core.exports.converter_yuv420p(this.converter, block.ptr, block.len, out.ptr);
  }

  /** The frame in `block` at 1280 x 720 as RGB, into `out`. */
  rgb(block: CoreBlock, out: CoreBlock): void {
    this.core.exports.converter_rgb24(this.converter, block.ptr, block.len, out.ptr);
  }

  /** The converter is done. */
  free(): void {
    if (this.converter) this.core.exports.converter_free(this.converter);
    this.converter = 0;
  }
}
