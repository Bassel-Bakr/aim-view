/// <reference lib="webworker" />
// The review in the browser, in a worker: decodes the recording (Mediabunny, the browser's own decoder), turns each
// frame into the exact pixels ffmpeg gives Python (the core's converter), finds the targets with the detector model
// (onnxruntime-web, WebAssembly) and tracks them (the core's tracker, which also watches the excluded areas for
// pop-ups). python/review.py's track_model, step by step: the fixed map from the key frames, then every frame. Frames before time 0 are the edit list's pre-roll: ffmpeg
// drops them, so the review does too.
import {
  ALL_FORMATS,
  BlobSource,
  EncodedPacketSink,
  Input,
  VideoSample,
  VideoSampleSink,
} from 'mediabunny';
import * as ort from 'onnxruntime-web/wasm';
import { TrackFrame, Tracks } from '../../api';
import { Core, CoreBlock, matrixNumber } from './core';
import { ReviewMessage, ReviewRequest } from './review-messages';

const W = 1280;
const H = 720;
const PROGRESS_EVERY = 60;

const say = (m: ReviewMessage) => postMessage(m);

addEventListener('message', (e: MessageEvent<ReviewRequest>) => {
  review(e.data).catch((err: unknown) =>
    say({ kind: 'error', error: err instanceof Error ? err.message : String(err) }),
  );
});

/** A frame's planes written into the core's memory as YUV 4:2:0 (Y, U, V), whatever layout the decoder gave. */
async function writeI420(
  sample: VideoSample,
  core: Core,
  block: CoreBlock,
  scratch: Uint8Array,
): Promise<void> {
  const { width: w, height: h } = sample.visibleRect;
  const layout = await sample.copyTo(scratch);
  const out = core.bytes(block);
  const [yp, up] = layout;
  for (let r = 0; r < h; r++)
    out.set(scratch.subarray(yp.offset + r * yp.stride, yp.offset + r * yp.stride + w), r * w);
  const cw = w >> 1;
  const ch = h >> 1;
  const u0 = w * h;
  const v0 = u0 + cw * ch;
  if (sample.format === 'I420') {
    const vp = layout[2];
    for (let r = 0; r < ch; r++) {
      out.set(
        scratch.subarray(up.offset + r * up.stride, up.offset + r * up.stride + cw),
        u0 + r * cw,
      );
      out.set(
        scratch.subarray(vp.offset + r * vp.stride, vp.offset + r * vp.stride + cw),
        v0 + r * cw,
      );
    }
  } else if (sample.format === 'NV12') {
    for (let r = 0; r < ch; r++) {
      const row = up.offset + r * up.stride;
      for (let c = 0; c < cw; c++) {
        out[u0 + r * cw + c] = scratch[row + 2 * c];
        out[v0 + r * cw + c] = scratch[row + 2 * c + 1];
      }
    }
  } else {
    throw new Error(
      `The decoder gave ${sample.format ?? 'an unknown'} frames; the review reads 8-bit YUV 4:2:0`,
    );
  }
}

/** The recording's frame rate as ffprobe gives a constant one (OBS records whole frame rates). */
function frameRate(rate: number): number {
  return Math.abs(rate - Math.round(rate)) < 0.01 ? Math.round(rate) : rate;
}

async function review(req: ReviewRequest): Promise<void> {
  const start = performance.now();
  const core = await Core.load(req.coreUrl);
  ort.env.wasm.wasmPaths = req.ortPath;
  ort.env.wasm.numThreads = self.crossOriginIsolated
    ? Math.min(8, navigator.hardwareConcurrency)
    : 1;
  const session = await ort.InferenceSession.create(req.modelUrl, { executionProviders: ['wasm'] });

  const input = new Input({ formats: ALL_FORMATS, source: new BlobSource(req.file) });
  const track = await input.getPrimaryVideoTrack();
  if (!track) throw new Error('The file has no video');
  const fps = frameRate((await track.computePacketStats(240)).averagePacketRate);
  const total = Math.round(fps * (await track.computeDuration()));
  const samples = new VideoSampleSink(track);

  // the converter, made for the first frame's size and colours; the buffers it reads and fills
  let converter = 0;
  let size = 0;
  let yuv: CoreBlock | null = null;
  let scratch = new Uint8Array(0);
  const yuv720 = core.reserve((W * H * 3) / 2);
  const rgb = core.reserve(W * H * 3);
  const prepare = (s: VideoSample) => {
    const { width: w, height: h } = s.visibleRect;
    if (!converter) {
      converter = core.x.converter_new(
        w,
        h,
        matrixNumber(s.colorSpace.matrix),
        s.colorSpace.fullRange ? 1 : 0,
      );
      size = (w * h * 3) / 2;
      yuv = core.reserve(size);
    }
    if (scratch.length < s.allocationSize()) scratch = new Uint8Array(s.allocationSize());
    return yuv as CoreBlock;
  };

  // 1. the fixed map, from the key frames (ffmpeg -skip_frame nokey)
  say({ kind: 'progress', stage: 'looking', done: 0, total });
  const packets = new EncodedPacketSink(track);
  const fixedBuilder = core.x.fixed_new();
  let keyFrames = 0;
  for (let p = await packets.getFirstKeyPacket(); p; p = await packets.getNextKeyPacket(p)) {
    if (p.timestamp < 0) continue;
    const s = await samples.getSample(p.timestamp);
    if (!s) continue;
    const block = prepare(s);
    await writeI420(s, core, block, scratch);
    s.close();
    core.x.converter_yuv420p(converter, block.ptr, size, yuv720.ptr);
    core.x.fixed_add(fixedBuilder, yuv720.ptr);
    keyFrames++;
  }
  const fixedBlock = core.reserve(W * H);
  core.x.fixed_finish(fixedBuilder, fixedBlock.ptr);
  const fixed = core.bytes(fixedBlock).slice();
  core.free(fixedBlock);
  const fixedTensor = new ort.Tensor('uint8', fixed, [1, H, W]);

  // 2. every frame: the detector, then the tracker
  const gw = W / 4;
  const gh = H / 4;
  const score = core.reserve(gw * gh * 4);
  const reg = core.reserve(4 * gw * gh * 4);
  const tracker = core.x.tracker_new_kovobs(req.cap ?? 0);
  let n = 0;
  for await (const s of samples.samples()) {
    if (s.timestamp < 0) {
      s.close();
      continue;
    }
    const block = prepare(s);
    await writeI420(s, core, block, scratch);
    s.close();
    core.x.converter_rgb24(converter, block.ptr, size, rgb.ptr);
    core.x.tracker_watch(tracker, rgb.ptr);
    const out = await session.run({
      rgb: new ort.Tensor('uint8', core.bytes(rgb).slice(), [1, H, W, 3]),
      fixed: fixedTensor,
    });
    core.floats(score).set(out['score'].data as Float32Array);
    core.floats(reg).set(out['reg'].data as Float32Array);
    core.x.tracker_push_maps(tracker, score.ptr, reg.ptr, gw, gh);
    if (++n % PROGRESS_EVERY === 0) say({ kind: 'progress', stage: 'tracking', done: n, total });
  }
  say({ kind: 'progress', stage: 'linking', done: n, total });
  const frames = JSON.parse(core.takeText(core.x.tracker_finish(tracker))) as TrackFrame[];
  if (converter) core.x.converter_free(converter);
  const share = fixed.reduce((a, v) => a + v, 0) / fixed.length;
  const tracks: Tracks = { fps, frames, fixed: share, detector: 'onnxruntime-web' };
  say({ kind: 'done', tracks, seconds: (performance.now() - start) / 1000, keyFrames });
}
