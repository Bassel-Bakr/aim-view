/// <reference lib="webworker" />
// The review in the browser, in a worker: decodes the recording (Mediabunny, the browser's own decoder), turns each
// frame into the exact pixels ffmpeg gives Python (the core's converter), finds the targets with the detector model
// (onnxruntime-web, WebAssembly) and tracks them (the core's tracker, which also watches the excluded areas for
// pop-ups). python/review.py's track_model, step by step: the fixed map from the key frames, then every frame. Frames
// before time 0 are the edit list's pre-roll: ffmpeg drops them, so the review does too. Each frame also goes to the
// camera worker (the camera's turn and KovaaK's countdown bar, which a tracking run's review reads).
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
import { CameraLink } from './camera-link';
import { Core, CoreBlock, matrixNumber } from './core';
import { BrowserDevice, FrameFormat, ReviewMessage, ReviewRequest } from './review-messages';

/** WebGPU's flag constants, which TypeScript's worker library leaves out (it has WebGPU's types). */
declare const GPUBufferUsage: Readonly<
  Record<'MAP_READ' | 'COPY_DST' | 'STORAGE', GPUBufferUsageFlags>
>;
declare const GPUMapMode: Readonly<Record<'READ', GPUMapModeFlags>>;

/** onnxruntime-web, either build: for the GPU (WebGPU) or the CPU (WebAssembly). Both have the same API. */
type Ort = typeof import('onnxruntime-web/wasm');

/**
 * The detector: onnxruntime-web's build, its session, and where it runs. On the GPU its outputs stay there until read
 * back, so the next call can be sent while one call's maps come back; `capture` says the session records its GPU work
 * (graph capture).
 */
interface Detector {
  ort: Ort;
  session: InferenceSession;
  device: BrowserDevice;
  capture: boolean;
}

/** One call's output maps, copied out of the GPU to be read back while the next call runs. */
interface Staging {
  score: GPUBuffer;
  reg: GPUBuffer;
}

/**
 * Graph capture: onnxruntime-web records the detector's GPU work on the first call and replays it on the next ones,
 * which takes most of the CPU's work out of a call. The inputs sit in fixed GPU buffers, written before each call. The
 * outputs are the buffers onnxruntime made for the first call, given back on every later one: it releases the handles
 * of outputs the caller makes, and a replayed call then fails to find them (onnxruntime-web 1.30). Each call's outputs
 * are copied to a staging pair before the next call is sent.
 */
interface Captured {
  device: GPUDevice;
  rgb: GPUBuffer;
  feeds: InferenceSession.FeedsType;
  outputs: InferenceSession.ReturnType;
  staging: Staging[];
  turn: number;
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

/**
 * The GPU session's settings: convolutions in NHWC, no extra validation, and the input size fixed to `batch` frames of
 * 1280 x 720 (every call then sends a full batch). The same tracks as the defaults, and faster: av1's first 900 frames
 * at 225 frames a second with the defaults, 251 with these, 279 with graph capture as well (the medians of 3 rounds in
 * test_out/browser_check/profile-runs.html).
 */
function gpuOptions(batch: number, capture: boolean): InferenceSession.SessionOptions {
  return {
    executionProviders: [{ name: 'webgpu', preferredLayout: 'NHWC', validationMode: 'disabled' }],
    preferredOutputLocation: 'gpu-buffer',
    freeDimensionOverrides: { n: batch, h: H, w: W },
    enableGraphCapture: capture,
  };
}

/**
 * The detector on the device asked for; on the CPU when the GPU cannot start. On the GPU with graph capture, unless the
 * model cannot have it (a node onnxruntime keeps on the CPU).
 */
async function startDetector(req: ReviewRequest): Promise<Detector> {
  if (req.device === 'webgpu') {
    try {
      const ort = await loadOrt('webgpu', req.ortPath);
      const batch = Math.max(1, req.batch);
      let capture = true;
      const session = await ort.InferenceSession.create(
        req.modelUrl,
        gpuOptions(batch, true),
      ).catch(() => {
        capture = false;
        return ort.InferenceSession.create(req.modelUrl, gpuOptions(batch, false));
      });
      return { ort, session, device: 'webgpu', capture };
    } catch {
      // no GPU the browser can use: the CPU
    }
  }
  const ort = await loadOrt('wasm', req.ortPath);
  const session = await ort.InferenceSession.create(req.modelUrl, { executionProviders: ['wasm'] });
  return { ort, session, device: 'wasm', capture: false };
}

/**
 * Graph capture's buffers for a session made with it, and its first two calls (the capture, then a replay), so a
 * capture that does not work shows before the review starts. `fixed` is the fixed map once for each frame of a batch.
 */
async function startCapture(
  ort: Ort,
  session: InferenceSession,
  fixed: Uint8Array,
  batch: number,
): Promise<Captured> {
  const device = await ort.env.webgpu.device;
  const input = GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_DST;
  const rgb = device.createBuffer({ size: batch * W * H * 3, usage: input });
  const fixedBuffer = device.createBuffer({ size: fixed.length, usage: input });
  device.queue.writeBuffer(fixedBuffer, 0, fixed);
  const feeds = {
    rgb: ort.Tensor.fromGpuBuffer(rgb, { dataType: 'uint8', dims: [batch, H, W, 3] }),
    fixed: ort.Tensor.fromGpuBuffer(fixedBuffer, { dataType: 'uint8', dims: [batch, H, W] }),
  };
  const outputs = await session.run(feeds);
  await session.run(feeds, outputs);
  const read = GPUBufferUsage.MAP_READ | GPUBufferUsage.COPY_DST;
  const stage = (name: string) =>
    device.createBuffer({ size: outputs[name].gpuBuffer.size, usage: read });
  const staging = [0, 1].map(() => ({ score: stage('score'), reg: stage('reg') }));
  return { device, rgb, feeds, outputs, staging, turn: 0 };
}

async function review(req: ReviewRequest): Promise<void> {
  const start = performance.now();
  const core = await Core.load(req.coreUrl);
  const detector = await startDetector(req);
  const { ort, device } = detector;
  let session = detector.session;

  const input = new Input({ formats: ALL_FORMATS, source: new BlobSource(req.file) });
  const track = await input.getPrimaryVideoTrack();
  if (!track) throw new Error('The file has no video');
  const fps = frameRate((await track.computePacketStats(240)).averagePacketRate);
  const total = Math.round(fps * (await track.computeDuration()));
  const samples = new VideoSampleSink(track, await decoderOptions(track));

  // the converter, made for the first frame's size and colours; the buffers it reads and fills
  let converter = 0;
  // set inside prepare(): "as" keeps TypeScript from taking it for null for good
  let format = null as FrameFormat | null;
  let size = 0;
  let yuv: CoreBlock | null = null;
  let scratch = new Uint8Array(0);
  const yuv720 = core.reserve((W * H * 3) / 2);
  const rgb = core.reserve(W * H * 3);
  const prepare = (s: VideoSample) => {
    const { width: w, height: h } = s.visibleRect;
    if (!converter) {
      format = {
        width: w,
        height: h,
        matrix: matrixNumber(s.colorSpace.matrix),
        full: s.colorSpace.fullRange ? 1 : 0,
      };
      converter = core.x.converter_new(w, h, format.matrix, format.full);
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
  if (!format) throw new Error('The video has no frames');
  const camera = new CameraLink(req.camera);
  camera.start({ kind: 'start', coreUrl: req.coreUrl, fixed, ...format });
  // the fixed map once for each frame of a call: the detector takes up to req.batch frames at once
  const fixedAll = (k: number) => {
    const all = new Uint8Array(k * W * H);
    for (let i = 0; i < k; i++) all.set(fixed, i * W * H);
    return all;
  };
  const fixedTensors = new Map<number, InstanceType<Ort['Tensor']>>();
  const fixedFor = (k: number) => {
    let t = fixedTensors.get(k);
    if (!t) {
      t = new ort.Tensor('uint8', fixedAll(k), [k, H, W]);
      fixedTensors.set(k, t);
    }
    return t;
  };
  const batch = Math.max(1, req.batch);
  let captured: Captured | null = null;
  if (detector.capture) {
    try {
      captured = await startCapture(ort, session, fixedAll(batch), batch);
    } catch {
      // the capture does not work here: the same session's settings without it
      await session.release();
      session = await ort.InferenceSession.create(req.modelUrl, gpuOptions(batch, false));
    }
  }

  // 2. every frame: the detector, then the tracker. The detector takes req.batch frames in one call (faster on most
  // GPUs, the same boxes); while it works on them, the next ones are decoded and converted. One call is sent at a time
  // (the WebGPU build cannot run two at once); on the GPU a call returns once it is queued and its maps are read back
  // after, so two can be on their way. The tracker takes each frame's maps in order.
  const gw = W / 4;
  const gh = H / 4;
  const score = core.reserve(gw * gh * 4);
  const reg = core.reserve(4 * gw * gh * 4);
  const tracker = core.x.tracker_new_kovobs(req.cap ?? 0);
  let n = 0;
  const lumaBytes = format.width * format.height;
  const rows = core.x.camera_rgb_rows();
  const [rowsStart, rowsEnd] = [(rows & 0xffff) * W * 3, (rows >> 16) * W * 3];
  const frameBytes = W * H * 3;
  const mapFloats = gw * gh;
  const onGpu = device === 'webgpu';
  const depth = onGpu ? 2 : 1;
  // the frames for the next call, each copied once into its place in the call's input
  let waiting = new Uint8Array(batch * frameBytes);
  let count = 0;
  let sending: Promise<unknown> = Promise.resolve();
  /** A captured call: its input written, the call sent, and its outputs copied out before the next call is sent. */
  const detectCaptured = (c: Captured, all: Uint8Array, k: number): Promise<Float32Array[]> => {
    const sent = sending.then(async () => {
      c.device.queue.writeBuffer(c.rgb, 0, all);
      await session.run(c.feeds, c.outputs);
      const st = c.staging[c.turn++ % c.staging.length];
      const copy = c.device.createCommandEncoder();
      copy.copyBufferToBuffer(c.outputs['score'].gpuBuffer, 0, st.score, 0, st.score.size);
      copy.copyBufferToBuffer(c.outputs['reg'].gpuBuffer, 0, st.reg, 0, st.reg.size);
      c.device.queue.submit([copy.finish()]);
      return st;
    });
    sending = sent.catch(() => undefined);
    return sent.then(async (st) => {
      await Promise.all([st.score.mapAsync(GPUMapMode.READ), st.reg.mapAsync(GPUMapMode.READ)]);
      const maps = [
        new Float32Array(st.score.getMappedRange(0, k * mapFloats * 4).slice(0)),
        new Float32Array(st.reg.getMappedRange(0, k * 4 * mapFloats * 4).slice(0)),
      ];
      st.score.unmap();
      st.reg.unmap();
      return maps;
    });
  };
  /**
   * One call's score and reg maps, read back, for the first k frames of `all` (a whole batch's buffer). On the GPU a
   * call always takes the whole batch (its input size is fixed); the frames after k are left over and give no maps.
   */
  const detect = (all: Uint8Array, k: number): Promise<Float32Array[]> => {
    if (captured) return detectCaptured(captured, all, k);
    const n = onGpu ? batch : k;
    const feeds = {
      rgb: new ort.Tensor('uint8', all.subarray(0, n * frameBytes), [n, H, W, 3]),
      fixed: fixedFor(n),
    };
    const sent = sending.then(() => session.run(feeds));
    sending = sent.catch(() => undefined);
    return sent.then(async (out) =>
      onGpu
        ? [
            ((await out['score'].getData(true)) as Float32Array).subarray(0, k * mapFloats),
            ((await out['reg'].getData(true)) as Float32Array).subarray(0, k * 4 * mapFloats),
          ]
        : [out['score'].data as Float32Array, out['reg'].data as Float32Array],
    );
  };
  /** A call's maps to the tracker, frame by frame. */
  const toTracker = ([scores, regs]: Float32Array[]) => {
    for (let i = 0; i < scores.length / (gw * gh); i++) {
      core.floats(score).set(scores.subarray(i * gw * gh, (i + 1) * gw * gh));
      core.floats(reg).set(regs.subarray(i * 4 * gw * gh, (i + 1) * 4 * gw * gh));
      core.x.tracker_push_maps(tracker, score.ptr, reg.ptr, gw, gh);
      if (++n % PROGRESS_EVERY === 0) say({ kind: 'progress', stage: 'tracking', done: n, total });
    }
  };
  const inFlight: Promise<unknown>[] = [];
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
    // to the camera worker: the Y plane, and the rows of the RGB the countdown test reads
    const copy = await camera.take(lumaBytes + rowsEnd - rowsStart);
    new Uint8Array(copy).set(core.bytes(block).subarray(0, lumaBytes));
    new Uint8Array(copy).set(core.bytes(rgb).subarray(rowsStart, rowsEnd), lumaBytes);
    camera.send(copy);
    core.x.tracker_watch(tracker, rgb.ptr);
    waiting.set(core.bytes(rgb), count++ * frameBytes);
    if (count < batch) continue;
    while (inFlight.length >= depth) await inFlight.shift();
    const maps = detect(waiting, count);
    // a call that fails stops the review at the next wait; the chain says so again at the end
    inFlight.push(maps);
    detecting = detecting.then(() => maps).then(toTracker);
    detecting.catch(() => undefined);
    waiting = new Uint8Array(batch * frameBytes);
    count = 0;
  }
  await detecting;
  if (count) toTracker(await detect(waiting, count));
  say({ kind: 'progress', stage: 'linking', done: n, total });
  const framesText = core.takeText(core.x.tracker_finish(tracker));
  const frames = JSON.parse(framesText) as TrackFrame[];
  const readings = await camera.finish(framesText);
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
