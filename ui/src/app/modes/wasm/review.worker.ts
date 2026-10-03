/// <reference lib="webworker" />
// The review in the browser, in a worker: decodes the recording (Mediabunny, the browser's own decoder), turns each
// frame into the exact pixels ffmpeg gives Python (the core's converter), finds the targets with the detector model
// (onnxruntime-web, WebAssembly) and tracks them (the core's tracker, which also watches the excluded areas for
// pop-ups). python/review.py's track_model, step by step: the fixed map from the key frames, then every frame. Frames
// before time 0 are the edit list's pre-roll: ffmpeg drops them, so the review does too. Each frame also feeds the camera
// watch (the camera's turn and KovaaK's countdown bar, which a tracking run's review reads).
import {
  ALL_FORMATS,
  BlobSource,
  EncodedPacketSink,
  Input,
  InputVideoTrack,
  VideoSample,
  VideoSampleSink,
  VideoSinkDecoderOptions,
} from 'mediabunny';
import type { InferenceSession } from 'onnxruntime-web/wasm';
import { TrackFrame, Tracks } from '../../api';
import { Core, CoreBlock, matrixNumber } from './core';
import { BrowserDevice, ReviewMessage, ReviewRequest, VideoReadings } from './review-messages';

/** onnxruntime-web, either build: for the GPU (WebGPU) or the CPU (WebAssembly). Both have the same API. */
type Ort = typeof import('onnxruntime-web/wasm');

/** The detector: onnxruntime-web's build, its session, and where it runs. */
interface Detector {
  ort: Ort;
  session: InferenceSession;
  device: BrowserDevice;
}

const DEVICE_NAMES: Record<BrowserDevice, string> = { webgpu: 'WebGPU', wasm: 'WebAssembly' };

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

/**
 * The decoder to ask for: the browser's software decoder, where it has one for the video. A hardware decoder's frames
 * are on the GPU, and copying each one back takes longer than the software decoder does, while the detector waits for
 * the GPU (av1 at 2560x1440: 50 frames a second with the hardware decoder, 76 with the software one). Both give the
 * same bytes. Chrome has no software decoder for HEVC: there, the hardware one.
 */
async function decoderOptions(track: InputVideoTrack): Promise<VideoSinkDecoderOptions> {
  const config = await track.getDecoderConfig();
  if (!config) return {};
  const software = await VideoDecoder.isConfigSupported({
    ...config,
    hardwareAcceleration: 'prefer-software',
  }).catch(() => null);
  return software?.supported ? { hardwareAcceleration: 'prefer-software' } : {};
}

/** onnxruntime-web's build for the device, loading its WebAssembly from ortPath. */
async function loadOrt(device: BrowserDevice, ortPath: string): Promise<Ort> {
  const ort: Ort =
    device === 'webgpu'
      ? await import('onnxruntime-web/webgpu')
      : await import('onnxruntime-web/wasm');
  ort.env.wasm.wasmPaths = ortPath;
  ort.env.wasm.numThreads = self.crossOriginIsolated
    ? Math.min(8, navigator.hardwareConcurrency)
    : 1;
  return ort;
}

/** The detector on the device asked for; on the CPU when the GPU cannot start. */
async function startDetector(req: ReviewRequest): Promise<Detector> {
  if (req.device === 'webgpu') {
    try {
      const ort = await loadOrt('webgpu', req.ortPath);
      const session = await ort.InferenceSession.create(req.modelUrl, {
        executionProviders: ['webgpu'],
      });
      return { ort, session, device: 'webgpu' };
    } catch {
      // no GPU the browser can use: the CPU
    }
  }
  const ort = await loadOrt('wasm', req.ortPath);
  const session = await ort.InferenceSession.create(req.modelUrl, { executionProviders: ['wasm'] });
  return { ort, session, device: 'wasm' };
}

async function review(req: ReviewRequest): Promise<void> {
  const start = performance.now();
  const core = await Core.load(req.coreUrl);
  const { ort, session, device } = await startDetector(req);

  const input = new Input({ formats: ALL_FORMATS, source: new BlobSource(req.file) });
  const track = await input.getPrimaryVideoTrack();
  if (!track) throw new Error('The file has no video');
  const fps = frameRate((await track.computePacketStats(240)).averagePacketRate);
  const total = Math.round(fps * (await track.computeDuration()));
  const samples = new VideoSampleSink(track, await decoderOptions(track));

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
  const camera = core.x.camera_new(fixedBlock.ptr);
  core.free(fixedBlock);
  const fixedTensor = new ort.Tensor('uint8', fixed, [1, H, W]);

  // 2. every frame: the detector, then the tracker. While the detector works on a frame, the next one is decoded and
  // converted; the tracker takes each frame's maps in order, one frame in the detector at a time.
  const gw = W / 4;
  const gh = H / 4;
  const score = core.reserve(gw * gh * 4);
  const reg = core.reserve(4 * gw * gh * 4);
  const tracker = core.x.tracker_new_kovobs(req.cap ?? 0);
  let n = 0;
  const videoFrames = samples.samples()[Symbol.asyncIterator]();
  let next = videoFrames.next();
  let detecting: Promise<void> = Promise.resolve();
  for (;;) {
    const got = await next;
    if (got.done) break;
    next = videoFrames.next();
    const s = got.value;
    if (s.timestamp < 0) {
      s.close();
      continue;
    }
    const block = prepare(s);
    await writeI420(s, core, block, scratch);
    s.close();
    core.x.converter_rgb24(converter, block.ptr, size, rgb.ptr);
    core.x.converter_yuv420p(converter, block.ptr, size, yuv720.ptr);
    core.x.camera_add(camera, yuv720.ptr, rgb.ptr);
    core.x.tracker_watch(tracker, rgb.ptr);
    const feeds = {
      rgb: new ort.Tensor('uint8', core.bytes(rgb).slice(), [1, H, W, 3]),
      fixed: fixedTensor,
    };
    await detecting;
    detecting = session.run(feeds).then((out) => {
      core.floats(score).set(out['score'].data as Float32Array);
      core.floats(reg).set(out['reg'].data as Float32Array);
      core.x.tracker_push_maps(tracker, score.ptr, reg.ptr, gw, gh);
      if (++n % PROGRESS_EVERY === 0) say({ kind: 'progress', stage: 'tracking', done: n, total });
    });
  }
  await detecting;
  say({ kind: 'progress', stage: 'linking', done: n, total });
  const framesText = core.takeText(core.x.tracker_finish(tracker));
  const frames = JSON.parse(framesText) as TrackFrame[];
  const framesBytes = new TextEncoder().encode(framesText);
  const framesBlock = core.reserve(framesBytes.length);
  core.bytes(framesBlock).set(framesBytes);
  const readingsText = core.takeText(
    core.x.camera_finish(camera, framesBlock.ptr, framesBytes.length),
  );
  core.free(framesBlock);
  const readings = JSON.parse(readingsText) as VideoReadings;
  if (converter) core.x.converter_free(converter);
  const share = fixed.reduce((a, v) => a + v, 0) / fixed.length;
  const tracks: Tracks = {
    fps,
    frames,
    fixed: share,
    detector: `onnxruntime-web (${DEVICE_NAMES[device]})`,
  };
  say({ kind: 'done', tracks, readings, seconds: (performance.now() - start) / 1000, keyFrames });
}
