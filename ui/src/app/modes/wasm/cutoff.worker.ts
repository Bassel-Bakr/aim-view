/// <reference lib="webworker" />
// The pixels of a submitted cut-off's labels, in a worker (python/model/hand_crops.py: cutoff_crops): the fixed map
// from the key frames, as the review makes it, then each label's frame at 1280 x 720 RGB, cropped. The frames are
// decoded as the review worker decodes them (the browser's decoder, the frames before time 0 dropped) and converted by
// the core's converter, so they are ffmpeg's pixels. Python seeks to each frame with ffmpeg's -ss, which can land a
// frame off in OBS's files; here each crop is from its own frame.
import {
  ALL_FORMATS,
  BlobSource,
  EncodedPacketSink,
  Input,
  VideoSample,
  VideoSampleSink,
} from 'mediabunny';
import { Core, CoreBlock, matrixNumber } from './core';
import { CropPixels, CutoffReply, CutoffWork } from './cutoff-messages';

const W = 1280;
const H = 720;
/** A label's crop: 256 pixels square. */
const CROP = 256;

const say = (m: CutoffReply) => postMessage(m);

addEventListener('message', (e: MessageEvent<CutoffWork>) => {
  readCrops(e.data).catch((err: unknown) =>
    say({ kind: 'error', error: err instanceof Error ? err.message : String(err) }),
  );
});

/** A frame's planes as packed YUV 4:2:0 (Y, U, V), from the decoder's I420 or NV12. */
async function packedI420(s: VideoSample): Promise<Uint8Array<ArrayBuffer>> {
  const { width: w, height: h } = s.visibleRect;
  const [cw, ch] = [w >> 1, h >> 1];
  const out = new Uint8Array(w * h + 2 * cw * ch);
  if (s.format === 'I420') {
    await s.copyTo(out, {
      layout: [
        { offset: 0, stride: w },
        { offset: w * h, stride: cw },
        { offset: w * h + cw * ch, stride: cw },
      ],
      rect: s.visibleRect,
    });
    return out;
  }
  if (s.format !== 'NV12') {
    throw new Error(
      `The decoder gave ${s.format ?? 'an unknown'} frames; the labels read 8-bit YUV 4:2:0`,
    );
  }
  const scratch = new Uint8Array(s.allocationSize());
  const [yp, up] = await s.copyTo(scratch);
  for (let r = 0; r < h; r++)
    out.set(scratch.subarray(yp.offset + r * yp.stride, yp.offset + r * yp.stride + w), r * w);
  for (let r = 0; r < ch; r++) {
    const row = up.offset + r * up.stride;
    for (let c = 0; c < cw; c++) {
      out[w * h + r * cw + c] = scratch[row + 2 * c];
      out[w * h + cw * ch + r * cw + c] = scratch[row + 2 * c + 1];
    }
  }
  return out;
}

/** A square of `size` pixels with `channels` bytes each from a 1280-wide image, at (x0, y0). */
function cropOf(
  image: Uint8Array,
  channels: number,
  x0: number,
  y0: number,
): Uint8Array<ArrayBuffer> {
  const out = new Uint8Array(CROP * CROP * channels);
  for (let r = 0; r < CROP; r++) {
    const from = ((y0 + r) * W + x0) * channels;
    out.set(image.subarray(from, from + CROP * channels), r * CROP * channels);
  }
  return out;
}

async function readCrops(work: CutoffWork): Promise<void> {
  const input = new Input({ formats: ALL_FORMATS, source: new BlobSource(work.file) });
  const track = await input.getPrimaryVideoTrack();
  if (!track) throw new Error('The file has no video');
  const core = await Core.load(work.coreUrl);
  const samples = new VideoSampleSink(track);
  const packets = new EncodedPacketSink(track);
  // every frame's time from 0 on, in order: frame i of the tracks is the i-th
  const only = { metadataOnly: true };
  const times: number[] = [];
  for (let p = await packets.getFirstPacket(only); p; p = await packets.getNextPacket(p, only)) {
    if (p.timestamp >= 0) times.push(p.timestamp);
  }
  times.sort((a, b) => a - b);

  let converter = 0;
  let yuv: CoreBlock | null = null;
  const yuv720 = core.reserve((W * H * 3) / 2);
  const rgb = core.reserve(W * H * 3);
  /** The sample's planes in the core's memory, the converter made for the first sample's size and colours. */
  const load = async (s: VideoSample): Promise<CoreBlock> => {
    const planes = await packedI420(s);
    if (!converter) {
      const { width: w, height: h } = s.visibleRect;
      converter = core.x.converter_new(
        w,
        h,
        matrixNumber(s.colorSpace.matrix),
        s.colorSpace.fullRange ? 1 : 0,
      );
      yuv = core.reserve(planes.length);
    }
    const block = yuv as CoreBlock;
    core.bytes(block).set(planes);
    s.close();
    return block;
  };

  // the fixed map, from the key frames (ffmpeg -skip_frame nokey), as the review makes it
  const builder = core.x.fixed_new();
  for (let p = await packets.getFirstKeyPacket(); p; p = await packets.getNextKeyPacket(p)) {
    if (p.timestamp < 0) continue;
    const s = await samples.getSample(p.timestamp);
    if (!s) continue;
    const block = await load(s);
    core.x.converter_yuv420p(converter, block.ptr, block.len, yuv720.ptr);
    core.x.fixed_add(builder, yuv720.ptr);
  }
  const fixedBlock = core.reserve(W * H);
  core.x.fixed_finish(builder, fixedBlock.ptr);
  const fixed = core.bytes(fixedBlock).slice();
  core.free(fixedBlock);

  const crops: CropPixels[] = [];
  for (const c of work.crops) {
    const at = times[c.frame];
    const s = at === undefined ? null : await samples.getSample(at);
    if (!s) throw new Error(`The video has no frame ${c.frame}`);
    const block = await load(s);
    core.x.converter_rgb24(converter, block.ptr, block.len, rgb.ptr);
    crops.push({
      rgb: cropOf(core.bytes(rgb), 3, c.x0, c.y0),
      fixed: cropOf(fixed, 1, c.x0, c.y0),
    });
  }
  if (converter) core.x.converter_free(converter);
  say({ kind: 'read', crops });
}
