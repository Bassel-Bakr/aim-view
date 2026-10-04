/// <reference lib="webworker" />
// One run of a review in the browser, in a worker: it decodes the recording (Mediabunny, the browser's own decoder),
// turns each frame into the exact pixels ffmpeg gives (the core's converter) and runs the detector model
// (onnxruntime-web). The core's review session (src/session.rs, as the native review uses it) does the rest: it plans
// the runs, reads the key frames (the fixed map and the HUD's boxes), says what each frame is for, and tracks the run's
// frames. Frames before time 0 are the edit list's pre-roll: ffmpeg drops them, so the review does too. Each frame the
// run reads also goes to the camera worker, which feeds the session's watches (the camera's turn, KovaaK's countdown
// bar and the HUD). The area finder has a worker of its own (area-finder.worker.ts), which decodes the frames it reads
// as this one does (video-frames.ts, frame-converter.ts).
import { VideoSample } from 'mediabunny';
import type { InferenceSession } from 'onnxruntime-web/wasm';
import { CameraLink } from './camera-link';
import { Core, NEXT_FRAME } from './core';
import { FrameConverter } from './frame-converter';
import {
  BrowserDevice,
  ReviewMessage,
  ReviewRequest,
  ReviewSetup,
  VideoRun,
} from './review-messages';
import { VideoFrames } from './video-frames';

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

const W = 1280;
const H = 720;
const PROGRESS_EVERY = 60;

const say = (m: ReviewMessage) => postMessage(m);

addEventListener('message', (e: MessageEvent<ReviewRequest>) => {
  review(e.data).catch((err: unknown) =>
    say({ kind: 'error', error: err instanceof Error ? err.message : String(err) }),
  );
});

/** The recording's frame rate as ffprobe gives a constant one (OBS records whole frame rates). */
function frameRate(rate: number): number {
  return Math.abs(rate - Math.round(rate)) < 0.01 ? Math.round(rate) : rate;
}

/**
 * The model's settings file (python/model/MODEL_FILE.md), beside its export: detector_<name>.json for
 * detector_<name>_u8in.onnx. Null when the model has none: the tracker then takes today's values.
 */
async function modelSettings(modelUrl: string): Promise<string | null> {
  const url = modelUrl.replace(/_u8in\.onnx$/, '.json');
  if (url === modelUrl) return null;
  // asking for JSON: a server would send the app's page for a missing file asked for as any type
  const res = await fetch(url, { headers: { Accept: 'application/json' } });
  if (res.status === 404) {
    console.warn(`${url} is missing: the detector takes today's values`);
    return null;
  }
  if (!res.ok)
    throw new Error(`The model's settings file ${url} could not be read (${res.status})`);
  return res.text();
}

/** onnxruntime-web's build for the device, loading its WebAssembly from ortPath. */
async function loadOrt(device: BrowserDevice, ortPath: string): Promise<Ort> {
  const ort: Ort =
    device === 'webgpu'
      ? await import('onnxruntime-web/webgpu')
      : await import('onnxruntime-web/wasm');
  ort.env.wasm.wasmPaths = ortPath;
  // on the GPU no node runs on the CPU: more threads there only take cores from the decoder (one thread: 3% faster)
  ort.env.wasm.numThreads =
    device === 'webgpu' || !self.crossOriginIsolated
      ? 1
      : Math.min(8, navigator.hardwareConcurrency);
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

/**
 * One run of the recording (src/session.rs: `split_runs`): its tracking's and its watches' parts, which the page joins
 * with the other runs'. A run but the last also reads the next run's first frame, for the camera's turn into it.
 */
async function review(req: ReviewRequest): Promise<void> {
  const video = await VideoFrames.open(req.file, 'software');
  const fps = frameRate((await video.track.computePacketStats(240)).averagePacketRate);
  const { times, keys } = await video.frameTimes();
  const [core, settings] = await Promise.all([Core.load(req.coreUrl), modelSettings(req.modelUrl)]);
  const detector = await startDetector(req);
  const { ort, device } = detector;
  let session = detector.session;
  const camera = new CameraLink(req.camera);
  camera.open({ kind: 'open', coreUrl: req.coreUrl });
  // the rows of a frame's RGB the countdown test reads, which go to the camera worker after its Y plane
  const rows = core.x.camera_rgb_rows();
  const [rowsStart, rowsEnd] = [(rows & 0xffff) * W * 3, (rows >> 16) * W * 3];

  // the converter, made for the first frame's size and colors, and the buffers it fills
  const frames = new FrameConverter(core);
  const yuv720 = core.reserve((W * H * 3) / 2);
  const rgb = core.reserve(W * H * 3);
  // the first key frame gives the frames' format, which the review is set up with
  const keySamples = video.keySamples()[Symbol.asyncIterator]();
  const firstKey = await keySamples.next();
  if (firstKey.done) throw new Error('The video has no frames');
  let block = await frames.write(firstKey.value);
  const format = frames.format;
  if (!format) throw new Error('The video has no frames');
  const lumaBytes = format.width * format.height;
  const cameraBytes = lumaBytes + rowsEnd - rowsStart;
  const setup: ReviewSetup = {
    fps,
    times,
    keys,
    format,
    cap: req.cap ?? 0,
    areas: req.areas,
    // JSON has no Infinity: an open end is the largest number instead
    window: req.window && {
      start: req.window.start,
      end: Math.min(req.window.end, Number.MAX_VALUE),
    },
    runs: req.runs,
  };
  const setupText = JSON.stringify(setup);
  const plan = core.review(setupText);
  if (settings !== null) core.setModel(plan, settings);
  const runs = JSON.parse(core.takeText(core.x.review_runs(plan))) as VideoRun[];
  const run = runs[req.run];
  if (!run) {
    await keySamples.return?.(undefined);
    say({ kind: 'part', part: null });
    return;
  }
  const total = runs.reduce((a, r) => a + r.frames, 0);

  // 1. the key frames (ffmpeg -skip_frame nokey): the fixed map and the HUD's boxes
  say({ kind: 'progress', stage: 'looking', done: 0, total });
  const keyPass = core.x.review_keys(plan);
  for (;;) {
    frames.yuv720(block, yuv720);
    core.x.keys_add(keyPass, yuv720.ptr, block.ptr, lumaBytes);
    const next = await keySamples.next();
    if (next.done) break;
    block = await frames.write(next.value);
  }
  const fixedBlock = core.reserve(W * H);
  const hud = core.takeText(core.x.keys_finish(keyPass, fixedBlock.ptr));
  const fixed = core.bytes(fixedBlock).slice();
  core.free(fixedBlock);
  camera.start({ kind: 'start', setup: setupText, run: req.run, fixed, hud });
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
  const tracking = core.x.review_tracking(plan, req.run);
  let n = 0;
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
  /** A call's maps to the run's tracking, frame by frame. */
  const toTracker = ([scores, regs]: Float32Array[]) => {
    for (let i = 0; i < scores.length / (gw * gh); i++) {
      core.floats(score).set(scores.subarray(i * gw * gh, (i + 1) * gw * gh));
      core.floats(reg).set(regs.subarray(i * 4 * gw * gh, (i + 1) * 4 * gw * gh));
      core.x.tracking_maps(tracking, score.ptr, reg.ptr, gw, gh);
      if (++n % PROGRESS_EVERY === 0) say({ kind: 'progress', stage: 'tracking', done: n, total });
    }
  };
  /** A frame to RGB, and to the camera worker: its Y plane, and the rows of the RGB the countdown test reads. */
  const convert = async (s: VideoSample) => {
    const block = await frames.write(s);
    frames.rgb(block, rgb);
    const copy = await camera.take(cameraBytes);
    new Uint8Array(copy).set(core.bytes(block).subarray(0, lumaBytes));
    new Uint8Array(copy).set(core.bytes(rgb).subarray(rowsStart, rowsEnd), lumaBytes);
    camera.send(copy);
  };
  const inFlight: Promise<unknown>[] = [];
  // the frames the run reads (its own, then but for the last run the next run's first); decoding stops at the frame
  // after them (a frame within half a frame of its time)
  const half = 0.5 / fps;
  const after = times[run.first + run.frames + (run.to === null ? 0 : 1)];
  const videoFrames = video.samples
    .samples(run.from, after === undefined ? Infinity : after - half)
    [Symbol.asyncIterator]();
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
    // what the session says the frame is for: the run's (track it), the next run's first (the watches only), or past
    // the run (stop)
    const use = core.x.tracking_next(tracking);
    if (use === NEXT_FRAME.stop) {
      s.close();
      const rest = await next;
      if (!rest.done) rest.value.close();
      await videoFrames.return?.();
      break;
    }
    await convert(s);
    if (use === NEXT_FRAME.watch) continue;
    core.x.tracking_watch(tracking, rgb.ptr);
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
  const track = core.takeOutcome(core.x.tracking_part(tracking));
  const watch = await camera.finish();
  frames.free();
  say({ kind: 'part', part: { setup: setupText, track, watch, fixed, device } });
}
