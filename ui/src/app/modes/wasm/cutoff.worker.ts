/// <reference lib="webworker" />
/**
 * The pixels of a submitted cut-off's labels, in a worker (python/model/hand_crops.py:
 * cutoff_crops): the fixed map from the key frames, as the review makes it, then each label's
 * frame at 1280 x 720 RGB, cropped. The frames are decoded as the review worker decodes them (the
 * browser's decoder, the frames before time 0 dropped) and converted by the core's converter, so
 * they are ffmpeg's pixels. Python seeks to each frame with ffmpeg's -ss, which can land a frame
 * off in OBS's files; here each crop is from its own frame. In: a `CutoffWork` from
 * browser-faint-cutoffs.ts. Out: one `CutoffReply`, every crop's RGB and fixed map.
 */
import { VideoSample } from 'mediabunny';
import {
  Core,
  CoreBlock,
  FRAME_PIXELS,
  FRAME_RGB_BYTES,
  FRAME_WIDTH_PX,
  FRAME_YUV420_BYTES,
  matrixNumber,
  RGB_CHANNELS,
} from './core';
import { CropPixels, CutoffReply, CutoffWork } from './cutoff-messages';
import { i420Layout, packNv12, unreadableFormat } from './frame-converter';
import { VideoFrames } from './video-frames';

/** A label's crop: 256 pixels square. */
const CROP = 256;
/** The fixed map's bytes a pixel. */
const FIXED_CHANNELS = 1;

/** Sends the answer to the page. */
const say = (reply: CutoffReply) => postMessage(reply);

addEventListener('message', (event: MessageEvent<CutoffWork>) => {
  readCrops(event.data).catch((error: unknown) =>
    say({ kind: 'error', error: error instanceof Error ? error.message : String(error) }),
  );
});

/**
 * A frame's planes as packed YUV 4:2:0 (Y, U, V), from the decoder's I420 or NV12. Throws for any
 * other format.
 */
async function packedI420(sample: VideoSample): Promise<Uint8Array<ArrayBuffer>> {
  const { width, height } = sample.visibleRect;
  const chromaBytes = (width >> 1) * (height >> 1);
  const out = new Uint8Array(width * height + 2 * chromaBytes);
  if (sample.format === 'I420') {
    await sample.copyTo(out, { layout: i420Layout(width, height), rect: sample.visibleRect });
    return out;
  }
  if (sample.format !== 'NV12') throw unreadableFormat(sample, 'the labels read');
  const scratch = new Uint8Array(sample.allocationSize());
  const planes = await sample.copyTo(scratch);
  packNv12(scratch, planes, out, width, height);
  return out;
}

/**
 * A square of `CROP` pixels with `channels` bytes each from a 1280-wide image, its top left corner
 * at (x0, y0) in pixels.
 */
function cropOf(
  image: Uint8Array,
  channels: number,
  x0: number,
  y0: number,
): Uint8Array<ArrayBuffer> {
  const out = new Uint8Array(CROP * CROP * channels);
  for (let row = 0; row < CROP; row++) {
    const from = ((y0 + row) * FRAME_WIDTH_PX + x0) * channels;
    out.set(image.subarray(from, from + CROP * channels), row * CROP * channels);
  }
  return out;
}

/**
 * The labels' frames in the core: its converter, made for the first frame's size and colors, and
 * the block each frame's planes are written into.
 */
class PackedFrames {
  /** The converter; 0 until the first frame. */
  converter = 0;
  /** The core memory each frame's planes are written into, sized for the first frame. */
  private block: CoreBlock | null = null;

  /** Writes into the given core's memory, with its converter. */
  constructor(private readonly core: Core) {}

  /** The sample's planes in the core's memory, the sample closed. */
  async load(sample: VideoSample): Promise<CoreBlock> {
    const planes = await packedI420(sample);
    if (!this.converter) {
      const { width, height } = sample.visibleRect;
      const matrix = matrixNumber(sample.colorSpace.matrix);
      const full = sample.colorSpace.fullRange ? 1 : 0;
      this.converter = this.core.exports.converter_new(width, height, matrix, full);
      this.block = this.core.reserve(planes.length);
    }
    const block = this.block as CoreBlock;
    this.core.bytes(block).set(planes);
    sample.close();
    return block;
  }

  /** Frees the converter. */
  free(): void {
    if (this.converter) this.core.exports.converter_free(this.converter);
  }
}

/** The fixed map from the key frames (ffmpeg -skip_frame nokey), as the review makes it. */
async function fixedMap(
  core: Core,
  video: VideoFrames,
  frames: PackedFrames,
  yuv720: CoreBlock,
): Promise<Uint8Array> {
  const builder = core.exports.fixed_new();
  for await (const sample of video.keySamples()) {
    const block = await frames.load(sample);
    core.exports.converter_yuv420p(frames.converter, block.ptr, block.len, yuv720.ptr);
    core.exports.fixed_add(builder, yuv720.ptr);
  }
  const fixedBlock = core.reserve(FRAME_PIXELS);
  core.exports.fixed_finish(builder, fixedBlock.ptr);
  const fixed = core.bytes(fixedBlock).slice();
  core.free(fixedBlock);
  return fixed;
}

/**
 * Reads every crop the page asked for and says them, in its order. Rejects when the video lacks a
 * frame asked for.
 */
async function readCrops(work: CutoffWork): Promise<void> {
  const video = await VideoFrames.open(work.file, 'any');
  const core = await Core.load(work.coreUrl);
  // every frame's time from 0 on, in order: frame i of the tracks is the i-th
  const { times } = await video.frameTimes();
  const frames = new PackedFrames(core);
  const yuv720 = core.reserve(FRAME_YUV420_BYTES);
  const rgb = core.reserve(FRAME_RGB_BYTES);
  const fixed = await fixedMap(core, video, frames, yuv720);
  const crops: CropPixels[] = [];
  for (const crop of work.crops) {
    const at = times[crop.frame];
    const sample = at === undefined ? null : await video.samples.getSample(at);
    if (!sample) throw new Error(`The video has no frame ${crop.frame}`);
    const block = await frames.load(sample);
    core.exports.converter_rgb24(frames.converter, block.ptr, block.len, rgb.ptr);
    crops.push({
      rgb: cropOf(core.bytes(rgb), RGB_CHANNELS, crop.x0, crop.y0),
      fixed: cropOf(fixed, FIXED_CHANNELS, crop.x0, crop.y0),
    });
  }
  frames.free();
  say({ kind: 'read', crops });
}
