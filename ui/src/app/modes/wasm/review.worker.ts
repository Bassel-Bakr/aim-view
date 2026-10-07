/// <reference lib="webworker" />
/**
 * One run of a review in the browser, in a worker: it decodes the recording (Mediabunny, the
 * browser's own decoder), turns each frame into the exact pixels ffmpeg gives (the core's
 * converter) and runs the detector model (onnxruntime-web). The core's review session
 * (src/session.rs, as the native review uses it) does the rest: it plans the runs, reads the key
 * frames (the fixed map and the HUD's boxes), says what each frame is for, and tracks the run's
 * frames. Frames before time 0 are the edit list's pre-roll: ffmpeg drops them, so the review does
 * too. Each frame the run reads also goes to the camera worker, which feeds the session's watches
 * (the camera's turn, KovaaK's countdown bar and the HUD). The area finder has a worker of its own
 * (area-finder.worker.ts), which decodes the frames it reads as this one does (video-frames.ts,
 * frame-converter.ts). In: a `ReviewRequest` from browser-review.ts. Out: progress messages, then
 * the run's part (`ReviewPart`), which the page joins with the other runs'.
 */
import { VideoSample } from 'mediabunny';
import type { InferenceSession } from 'onnxruntime-web/wasm';
import { CameraLink } from './camera-link';
import {
  Core,
  CoreBlock,
  FRAME_HEIGHT_PX,
  FRAME_PIXELS,
  FRAME_RGB_BYTES,
  FRAME_WIDTH_PX,
  FRAME_YUV420_BYTES,
  NEXT_FRAME,
  RGB_CHANNELS,
} from './core';
import { FrameConverter } from './frame-converter';
import {
  BrowserDevice,
  FrameFormat,
  ReviewMessage,
  ReviewRequest,
  ReviewSetup,
  VideoRun,
} from './review-messages';
import { FrameTimes, VideoFrames } from './video-frames';

/**
 * WebGPU's buffer usage flags, which TypeScript's worker library leaves out (it has WebGPU's
 * types).
 */
declare const GPUBufferUsage: Readonly<
  Record<'MAP_READ' | 'COPY_DST' | 'STORAGE', GPUBufferUsageFlags>
>;
/** WebGPU's map mode flags, left out for the same reason. */
declare const GPUMapMode: Readonly<Record<'READ', GPUMapModeFlags>>;

/**
 * onnxruntime-web, either build: for the GPU (WebGPU) or the CPU (WebAssembly). Both have the same
 * API.
 */
type Ort = typeof import('onnxruntime-web/wasm');

/**
 * The detector: onnxruntime-web's build, its session, and where it runs. On the GPU its outputs
 * stay there until read back, so the next call can be sent while one call's maps come back;
 * `capture` says the session records its GPU work (graph capture).
 */
interface Detector {
  /** The onnxruntime-web build loaded for the device. */
  ort: Ort;
  /** The model's session. */
  session: InferenceSession;
  /** Where it runs: the GPU, or the CPU when the GPU could not start. */
  device: BrowserDevice;
  /** The session was made with graph capture. */
  capture: boolean;
}

/** One call's output maps, copied out of the GPU to be read back while the next call runs. */
interface Staging {
  /** A mappable copy of the score map. */
  score: GPUBuffer;
  /** A mappable copy of the reg map. */
  reg: GPUBuffer;
}

/**
 * Graph capture: onnxruntime-web records the detector's GPU work on the first call and replays it
 * on the next ones, which takes most of the CPU's work out of a call. The inputs sit in fixed GPU
 * buffers, written before each call. The outputs are the buffers onnxruntime made for the first
 * call, given back on every later one: it releases the handles of outputs the caller makes, and a
 * replayed call then fails to find them (onnxruntime-web 1.30). Each call's outputs are copied to
 * a staging pair before the next call is sent.
 */
interface Captured {
  /** The GPU device onnxruntime runs on. */
  device: GPUDevice;
  /** The fixed input buffer each call's frames are written into, a batch of 720p RGB. */
  rgb: GPUBuffer;
  /** The inputs, as tensors over the fixed GPU buffers (rgb and the fixed map). */
  feeds: InferenceSession.FeedsType;
  /** The output buffers onnxruntime made for the first call, given back on every call. */
  outputs: InferenceSession.ReturnType;
  /** Two staging pairs, used in turn. */
  staging: Staging[];
  /** The calls made so far, which picks the next staging pair. */
  turn: number;
}

/** How often the review says how far it is, in frames. */
const PROGRESS_EVERY = 60;
/** The packets the frame rate is measured over. */
const RATE_PACKETS = 240;
/** How far a frame rate may be from a whole number and still be taken as one (frames a second). */
const WHOLE_RATE_TOLERANCE = 0.01;
/** The most threads onnxruntime takes on the CPU. */
const CPU_MAX_THREADS = 8;
/** The detector's maps are a quarter of the frame each way: one cell for each 4 x 4 pixels. */
const MAP_SCALE = 4;
/** The maps' width in cells. */
const MAP_WIDTH = FRAME_WIDTH_PX / MAP_SCALE;
/** The maps' height in cells. */
const MAP_HEIGHT = FRAME_HEIGHT_PX / MAP_SCALE;
/** The cells in one frame's score map. */
const MAP_CELLS = MAP_WIDTH * MAP_HEIGHT;
/** The reg map's values for each cell (the box's four sides). */
const REG_VALUES = 4;
/** A 32-bit float's bytes. */
const FLOAT_BYTES = 4;
/**
 * Detector calls on their way at once on the GPU: two (one queued while one is read back). The
 * CPU takes one.
 */
const GPU_CALLS_IN_FLIGHT = 2;
/**
 * camera_rgb_rows packs the countdown rows' first row in its low 16 bits and their end in its
 * high ones: the shift to the high ones.
 */
const ROW_BITS = 16;
/** Keeps the low 16 bits: the first row. */
const ROW_MASK = 0xffff;

/** Sends a message to the page. */
const say = (reply: ReviewMessage) => postMessage(reply);

addEventListener('message', (event: MessageEvent<ReviewRequest>) => {
  review(event.data).catch((error: unknown) =>
    say({ kind: 'error', error: error instanceof Error ? error.message : String(error) }),
  );
});

/** The recording's frame rate as ffprobe gives a constant one (OBS records whole frame rates). */
function frameRate(rate: number): number {
  return Math.abs(rate - Math.round(rate)) < WHOLE_RATE_TOLERANCE ? Math.round(rate) : rate;
}

/**
 * The model's settings file (python/model/MODEL_FILE.md), beside its export: detector_<name>.json
 * for detector_<name>_u8in.onnx. Null when the model has none: the tracker then takes today's
 * values. Rejects when the server answers with another error.
 */
async function modelSettings(modelUrl: string): Promise<string | null> {
  const url = modelUrl.replace(/_u8in\.onnx$/, '.json');
  if (url === modelUrl) return null;
  // asking for JSON: a server would send the app's page for a missing file asked for as any type
  const response = await fetch(url, { headers: { Accept: 'application/json' } });
  if (response.status === 404) {
    console.warn(`${url} is missing: the detector takes today's values`);
    return null;
  }
  if (!response.ok)
    throw new Error(`The model's settings file ${url} could not be read (${response.status})`);
  return response.text();
}

/** onnxruntime-web's build for the device, loading its WebAssembly from ortPath. */
async function loadOrt(device: BrowserDevice, ortPath: string): Promise<Ort> {
  const ort: Ort =
    device === 'webgpu'
      ? await import('onnxruntime-web/webgpu')
      : await import('onnxruntime-web/wasm');
  ort.env.wasm.wasmPaths = ortPath;
  // on the GPU no node runs on the CPU: more threads there only take cores from the decoder (one
  // thread: 3% faster)
  ort.env.wasm.numThreads =
    device === 'webgpu' || !self.crossOriginIsolated
      ? 1
      : Math.min(CPU_MAX_THREADS, navigator.hardwareConcurrency);
  return ort;
}

/**
 * The GPU session's settings: convolutions in NHWC, no extra validation, and the input size fixed
 * to `batch` frames of 1280 x 720 (every call then sends a full batch). The same tracks as the
 * defaults, and faster: av1's first 900 frames at 225 frames a second with the defaults, 251 with
 * these, 279 with graph capture as well (the medians of 3 rounds in
 * test_out/browser_check/profile-runs.html).
 */
function gpuOptions(batch: number, capture: boolean): InferenceSession.SessionOptions {
  return {
    executionProviders: [{ name: 'webgpu', preferredLayout: 'NHWC', validationMode: 'disabled' }],
    preferredOutputLocation: 'gpu-buffer',
    // eslint-disable-next-line id-length -- the model's own names for its input's dimensions
    freeDimensionOverrides: { n: batch, h: FRAME_HEIGHT_PX, w: FRAME_WIDTH_PX },
    enableGraphCapture: capture,
  };
}

/**
 * The detector on the device asked for; on the CPU when the GPU cannot start. On the GPU with
 * graph capture, unless the model cannot have it (a node onnxruntime keeps on the CPU).
 */
async function startDetector(request: ReviewRequest): Promise<Detector> {
  if (request.device === 'webgpu') {
    try {
      const ort = await loadOrt('webgpu', request.ortPath);
      const batch = Math.max(1, request.batch);
      let capture = true;
      const session = await ort.InferenceSession.create(
        request.modelUrl,
        gpuOptions(batch, true),
      ).catch(() => {
        capture = false;
        return ort.InferenceSession.create(request.modelUrl, gpuOptions(batch, false));
      });
      return { ort, session, device: 'webgpu', capture };
    } catch {
      // no GPU the browser can use: the CPU
    }
  }
  const ort = await loadOrt('wasm', request.ortPath);
  const session = await ort.InferenceSession.create(request.modelUrl, {
    executionProviders: ['wasm'],
  });
  return { ort, session, device: 'wasm', capture: false };
}

/** The fixed map once for each frame of a call. */
function fixedTimes(fixed: Uint8Array, frameCount: number): Uint8Array {
  const all = new Uint8Array(frameCount * FRAME_PIXELS);
  for (let i = 0; i < frameCount; i++) all.set(fixed, i * FRAME_PIXELS);
  return all;
}

/**
 * Graph capture's buffers for a session made with it, and its first two calls (the capture, then
 * a replay), so a capture that does not work shows before the review starts. `fixed` is the fixed
 * map once for each frame of a batch.
 */
async function startCapture(
  ort: Ort,
  session: InferenceSession,
  fixed: Uint8Array,
  batch: number,
): Promise<Captured> {
  const device = await ort.env.webgpu.device;
  const input = GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_DST;
  const rgb = device.createBuffer({ size: batch * FRAME_RGB_BYTES, usage: input });
  const fixedBuffer = device.createBuffer({ size: fixed.length, usage: input });
  device.queue.writeBuffer(fixedBuffer, 0, fixed);
  const feeds = {
    rgb: ort.Tensor.fromGpuBuffer(rgb, {
      dataType: 'uint8',
      dims: [batch, FRAME_HEIGHT_PX, FRAME_WIDTH_PX, RGB_CHANNELS],
    }),
    fixed: ort.Tensor.fromGpuBuffer(fixedBuffer, {
      dataType: 'uint8',
      dims: [batch, FRAME_HEIGHT_PX, FRAME_WIDTH_PX],
    }),
  };
  const outputs = await session.run(feeds);
  await session.run(feeds, outputs);
  const read = GPUBufferUsage.MAP_READ | GPUBufferUsage.COPY_DST;
  const stage = (name: string) =>
    device.createBuffer({ size: outputs[name].gpuBuffer.size, usage: read });
  const staging = [0, 1].map(() => ({ score: stage('score'), reg: stage('reg') }));
  return { device, rgb, feeds, outputs, staging, turn: 0 };
}

/** A captured call's score and reg maps for its first frames, read back from its staging pair. */
async function readStaging(staging: Staging, frameCount: number): Promise<Float32Array[]> {
  await Promise.all([
    staging.score.mapAsync(GPUMapMode.READ),
    staging.reg.mapAsync(GPUMapMode.READ),
  ]);
  const scoreBytes = frameCount * MAP_CELLS * FLOAT_BYTES;
  const maps = [
    new Float32Array(staging.score.getMappedRange(0, scoreBytes).slice(0)),
    new Float32Array(staging.reg.getMappedRange(0, REG_VALUES * scoreBytes).slice(0)),
  ];
  staging.score.unmap();
  staging.reg.unmap();
  return maps;
}

/**
 * The detector's calls for a run. One call is sent at a time (the WebGPU build cannot run two at
 * once); on the GPU a call returns once it is queued and its maps are read back after, so two can
 * be on their way.
 */
class DetectorCalls {
  /** The last call sent, which the next one waits for; it never rejects. */
  private sending: Promise<unknown> = Promise.resolve();
  /** The fixed map's input for a call of each frame count, made once. */
  private readonly fixedTensors = new Map<number, InstanceType<Ort['Tensor']>>();

  /**
   * Keeps the session and what each call needs: whether it runs on the GPU, the frames a call
   * takes, the fixed map (1280 x 720) and graph capture's buffers (null without it). Callers make
   * one with `start`.
   */
  private constructor(
    private readonly ort: Ort,
    private readonly session: InferenceSession,
    private readonly onGpu: boolean,
    readonly batch: number,
    private readonly fixed: Uint8Array,
    private readonly captured: Captured | null,
  ) {}

  /**
   * The calls, with graph capture where the detector has it and it works here; else the session
   * as it is.
   */
  static async start(
    detector: Detector,
    fixed: Uint8Array,
    batch: number,
    modelUrl: string,
  ): Promise<DetectorCalls> {
    const { ort } = detector;
    let session = detector.session;
    let captured: Captured | null = null;
    if (detector.capture) {
      try {
        captured = await startCapture(ort, session, fixedTimes(fixed, batch), batch);
      } catch {
        // the capture does not work here: the same session's settings without it
        await session.release();
        session = await ort.InferenceSession.create(modelUrl, gpuOptions(batch, false));
      }
    }
    const onGpu = detector.device === 'webgpu';
    return new DetectorCalls(ort, session, onGpu, batch, fixed, captured);
  }

  /**
   * One call's score and reg maps, read back, for the first frameCount frames of `all` (a whole
   * batch's buffer). On the GPU a call always takes the whole batch (its input size is fixed); the
   * frames after them are left over and give no maps.
   */
  detect(all: Uint8Array, frameCount: number): Promise<Float32Array[]> {
    if (this.captured) return this.detectCaptured(this.captured, all, frameCount);
    const inputFrames = this.onGpu ? this.batch : frameCount;
    const dims = [inputFrames, FRAME_HEIGHT_PX, FRAME_WIDTH_PX, RGB_CHANNELS];
    const feeds = {
      rgb: new this.ort.Tensor('uint8', all.subarray(0, inputFrames * FRAME_RGB_BYTES), dims),
      fixed: this.fixedFor(inputFrames),
    };
    const sent = this.sending.then(() => this.session.run(feeds));
    this.sending = sent.catch(() => undefined);
    return sent.then(async (out) => {
      if (!this.onGpu) return [out['score'].data as Float32Array, out['reg'].data as Float32Array];
      const scores = (await out['score'].getData(true)) as Float32Array;
      const regs = (await out['reg'].getData(true)) as Float32Array;
      return [
        scores.subarray(0, frameCount * MAP_CELLS),
        regs.subarray(0, frameCount * REG_VALUES * MAP_CELLS),
      ];
    });
  }

  /**
   * A captured call: its input written, the call sent, and its outputs copied out before the next
   * call is sent.
   */
  private detectCaptured(
    captured: Captured,
    all: Uint8Array,
    frameCount: number,
  ): Promise<Float32Array[]> {
    const sent = this.sending.then(async () => {
      captured.device.queue.writeBuffer(captured.rgb, 0, all);
      await this.session.run(captured.feeds, captured.outputs);
      const staging = captured.staging[captured.turn++ % captured.staging.length];
      const copy = captured.device.createCommandEncoder();
      const { score, reg } = captured.outputs;
      copy.copyBufferToBuffer(score.gpuBuffer, 0, staging.score, 0, staging.score.size);
      copy.copyBufferToBuffer(reg.gpuBuffer, 0, staging.reg, 0, staging.reg.size);
      captured.device.queue.submit([copy.finish()]);
      return staging;
    });
    this.sending = sent.catch(() => undefined);
    return sent.then((staging) => readStaging(staging, frameCount));
  }

  /** The fixed map's input tensor for a call of frameCount frames, made the first time. */
  private fixedFor(frameCount: number): InstanceType<Ort['Tensor']> {
    let tensor = this.fixedTensors.get(frameCount);
    if (!tensor) {
      const dims = [frameCount, FRAME_HEIGHT_PX, FRAME_WIDTH_PX];
      tensor = new this.ort.Tensor('uint8', fixedTimes(this.fixed, frameCount), dims);
      this.fixedTensors.set(frameCount, tensor);
    }
    return tensor;
  }
}

/** The run's tracking in the core, which takes each call's maps frame by frame, in order. */
class TrackerFeed {
  /** The core's handle of the run's tracking (`review_tracking`). */
  readonly tracking: number;
  /** The core memory one frame's score map is copied into. */
  private readonly score: CoreBlock;
  /** The core memory one frame's reg map is copied into. */
  private readonly reg: CoreBlock;
  /** The frames given to the tracking so far. */
  private tracked = 0;

  /**
   * Starts the tracking of run `run` of the review `plan`; total is the frames all the runs
   * track, for the progress messages.
   */
  constructor(
    private readonly core: Core,
    plan: number,
    run: number,
    private readonly total: number,
  ) {
    this.score = core.reserve(MAP_CELLS * FLOAT_BYTES);
    this.reg = core.reserve(REG_VALUES * MAP_CELLS * FLOAT_BYTES);
    this.tracking = core.exports.review_tracking(plan, run);
  }

  /** A call's maps, frame by frame, saying how far the run is every PROGRESS_EVERY frames. */
  take([scores, regs]: Float32Array[]): void {
    const regCells = REG_VALUES * MAP_CELLS;
    for (let i = 0; i < scores.length / MAP_CELLS; i++) {
      this.core.floats(this.score).set(scores.subarray(i * MAP_CELLS, (i + 1) * MAP_CELLS));
      this.core.floats(this.reg).set(regs.subarray(i * regCells, (i + 1) * regCells));
      const { score, reg } = this;
      this.core.exports.tracking_maps(this.tracking, score.ptr, reg.ptr, MAP_WIDTH, MAP_HEIGHT);
      if (++this.tracked % PROGRESS_EVERY === 0)
        say({ kind: 'progress', stage: 'tracking', done: this.tracked, total: this.total });
    }
  }
}

/**
 * The frames for the next detector call, each copied once into its place in the call's input, and
 * the calls on their way. While the detector works on a call, the next frames are decoded and
 * converted; the tracker takes each call's maps in order.
 */
class Batches {
  /** The next call's input: a batch of 720p RGB frames, filled from the start. */
  private waiting: Uint8Array;
  /** The frames in `waiting` so far. */
  private count = 0;
  /** The calls sent and not yet waited for, oldest first. */
  private readonly inFlight: Promise<unknown>[] = [];
  /** The chain that hands each call's maps to the tracker, in order. */
  private detecting: Promise<void> = Promise.resolve();

  /** Batches frames for the calls, with at most depth calls on their way. */
  constructor(
    private readonly calls: DetectorCalls,
    private readonly tracker: TrackerFeed,
    private readonly depth: number,
  ) {
    this.waiting = new Uint8Array(calls.batch * FRAME_RGB_BYTES);
  }

  /**
   * A frame's RGB; a full batch goes to the detector once fewer than `depth` calls are on their
   * way.
   */
  async add(rgb: Uint8Array): Promise<void> {
    this.waiting.set(rgb, this.count++ * FRAME_RGB_BYTES);
    if (this.count < this.calls.batch) return;
    while (this.inFlight.length >= this.depth) await this.inFlight.shift();
    const maps = this.calls.detect(this.waiting, this.count);
    // a call that fails stops the review at the next wait; the chain says so again at the end
    this.inFlight.push(maps);
    this.detecting = this.detecting.then(() => maps).then((found) => this.tracker.take(found));
    this.detecting.catch(() => undefined);
    this.waiting = new Uint8Array(this.calls.batch * FRAME_RGB_BYTES);
    this.count = 0;
  }

  /** Waits for the calls on their way, then detects the frames left over. */
  async finish(): Promise<void> {
    await this.detecting;
    if (this.count) this.tracker.take(await this.calls.detect(this.waiting, this.count));
  }
}

/**
 * The camera worker's share of a frame: its Y plane, then the rows of its RGB the countdown test
 * reads.
 */
interface CameraShare {
  /** The Y plane's bytes: the recording's width times its height. */
  lumaBytes: number;
  /** The byte offset in the 720p RGB where the countdown rows start. */
  rowsStart: number;
  /** The byte offset in the 720p RGB where they end. */
  rowsEnd: number;
}

/**
 * The camera worker's share of a frame of this format; `rows`: the countdown rows (the core's
 * camera_rgb_rows).
 */
function cameraShare(rows: number, format: FrameFormat): CameraShare {
  const rowBytes = FRAME_WIDTH_PX * RGB_CHANNELS;
  return {
    lumaBytes: format.width * format.height,
    rowsStart: (rows & ROW_MASK) * rowBytes,
    rowsEnd: (rows >> ROW_BITS) * rowBytes,
  };
}

/**
 * The decoding of a run's frames: the core, its converter, and the blocks a frame is converted
 * into.
 */
interface Decoding {
  /** This worker's core. */
  core: Core;
  /** Writes each decoded frame into the core and converts it. */
  frames: FrameConverter;
  /** The core memory a frame's 720p YUV 4:2:0 goes into. */
  yuv720: CoreBlock;
  /** The core memory a frame's 720p RGB goes into. */
  rgb: CoreBlock;
}

/**
 * A frame to RGB, and to the camera worker: its Y plane, and the rows of the RGB the countdown test
 * reads.
 */
async function convert(
  decoding: Decoding,
  camera: CameraLink,
  share: CameraShare,
  sample: VideoSample,
): Promise<void> {
  const { core, frames, rgb } = decoding;
  const block = await frames.write(sample);
  frames.rgb(block, rgb);
  const copy = await camera.take(share.lumaBytes + share.rowsEnd - share.rowsStart);
  new Uint8Array(copy).set(core.bytes(block).subarray(0, share.lumaBytes));
  new Uint8Array(copy).set(
    core.bytes(rgb).subarray(share.rowsStart, share.rowsEnd),
    share.lumaBytes,
  );
  camera.send(copy);
}

/** What the key frames give: the fixed map (1280 x 720) and where the HUD's boxes are (as JSON). */
interface KeysRead {
  /** The fixed map, a byte a pixel at 1280 x 720 (1 fixed). */
  fixed: Uint8Array;
  /** Where the HUD's boxes are, as JSON (keys_finish's). */
  hud: string;
}

/**
 * The key frames (ffmpeg -skip_frame nokey), the first already written into `first`: the fixed
 * map and the HUD's boxes.
 */
async function readKeys(
  decoding: Decoding,
  plan: number,
  keySamples: AsyncIterator<VideoSample>,
  first: CoreBlock,
  lumaBytes: number,
): Promise<KeysRead> {
  const { core, frames, yuv720 } = decoding;
  const keyPass = core.exports.review_keys(plan);
  let block = first;
  for (;;) {
    frames.yuv720(block, yuv720);
    core.exports.keys_add(keyPass, yuv720.ptr, block.ptr, lumaBytes);
    const next = await keySamples.next();
    if (next.done) break;
    block = await frames.write(next.value);
  }
  const fixedBlock = core.reserve(FRAME_PIXELS);
  const hud = core.takeText(core.exports.keys_finish(keyPass, fixedBlock.ptr));
  const fixed = core.bytes(fixedBlock).slice();
  core.free(fixedBlock);
  return { fixed, hud };
}

/**
 * The review's setup (src/session.rs: `Setup`), from the request and what the packets and the
 * first frame say.
 */
function reviewSetup(
  request: ReviewRequest,
  fps: number,
  frameTimes: FrameTimes,
  format: FrameFormat,
): ReviewSetup {
  return {
    fps,
    times: frameTimes.times,
    keys: frameTimes.keys,
    format,
    cap: request.cap ?? 0,
    areas: request.areas,
    // JSON has no Infinity: an open end is the largest number instead
    window: request.window && {
      start: request.window.start,
      end: Math.min(request.window.end, Number.MAX_VALUE),
    },
    runs: request.runs,
    kind: request.kind,
  };
}

/**
 * Where decoding a run stops, in seconds: half a frame before the frame after the frames it reads
 * (so that frame is not decoded); Infinity when the run reads to the end.
 */
function runEnd(run: VideoRun, times: number[], fps: number): number {
  const half = 0.5 / fps;
  const after = times[run.first + run.frames + (run.to === null ? 0 : 1)];
  return after === undefined ? Infinity : after - half;
}

/**
 * What a run's frames go to: the camera worker (its link and share), the run's tracking and the
 * detector's batches.
 */
interface RunSteps {
  /** The link to the camera worker. */
  camera: CameraLink;
  /** The bytes of each frame the camera worker gets. */
  share: CameraShare;
  /** The run's tracking in the core. */
  tracker: TrackerFeed;
  /** The detector's batches of tracked frames. */
  batches: Batches;
}

/**
 * The frames the run reads (its own, then but for the last run the next run's first), each
 * converted and sent to the camera worker, the run's own tracked, until the session says a frame
 * is past the run.
 */
async function readRun(
  videoFrames: AsyncGenerator<VideoSample, void, unknown>,
  decoding: Decoding,
  steps: RunSteps,
): Promise<void> {
  const { core, rgb } = decoding;
  const { tracking } = steps.tracker;
  let next = videoFrames.next();
  for (;;) {
    const got = await next;
    if (got.done) break;
    next = videoFrames.next();
    const sample = got.value;
    if (sample.timestamp < 0) {
      sample.close();
      continue;
    }
    // what the session says the frame is for: the run's (track it), the next run's first (the
    // watches only), or past the run (stop)
    const use = core.exports.tracking_next(tracking);
    if (use === NEXT_FRAME.stop) {
      sample.close();
      const rest = await next;
      if (!rest.done) rest.value.close();
      await videoFrames.return?.();
      break;
    }
    await convert(decoding, steps.camera, steps.share, sample);
    if (use === NEXT_FRAME.watch) continue;
    core.exports.tracking_watch(tracking, rgb.ptr);
    await steps.batches.add(core.bytes(rgb));
  }
}

/**
 * One run of the recording (src/session.rs: `split_runs`): its tracking's and its watches' parts,
 * which the page joins with the other runs'. A run but the last also reads the next run's first
 * frame, for the camera's turn into it. Says a null part when the recording has no such run.
 */
async function review(request: ReviewRequest): Promise<void> {
  const video = await VideoFrames.open(request.file, 'software');
  const fps = frameRate((await video.track.computePacketStats(RATE_PACKETS)).averagePacketRate);
  const frameTimes = await video.frameTimes();
  const [core, settings] = await Promise.all([
    Core.load(request.coreUrl),
    modelSettings(request.modelUrl),
  ]);
  const detector = await startDetector(request);
  const camera = new CameraLink(request.camera);
  camera.open({ kind: 'open', coreUrl: request.coreUrl });
  // the rows of a frame's RGB the countdown test reads, which go to the camera worker after its
  // Y plane
  const rows = core.exports.camera_rgb_rows();
  // the converter, made for the first frame's size and colors, and the buffers it fills
  const frames = new FrameConverter(core);
  const yuv720 = core.reserve(FRAME_YUV420_BYTES);
  const decoding: Decoding = { core, frames, yuv720, rgb: core.reserve(FRAME_RGB_BYTES) };
  // the first key frame gives the frames' format, which the review is set up with
  const keySamples = video.keySamples()[Symbol.asyncIterator]();
  const firstKey = await keySamples.next();
  if (firstKey.done) throw new Error('The video has no frames');
  const firstBlock = await frames.write(firstKey.value);
  const format = frames.format;
  if (!format) throw new Error('The video has no frames');
  const share = cameraShare(rows, format);
  const setup = JSON.stringify(reviewSetup(request, fps, frameTimes, format));
  const plan = core.review(setup);
  if (settings !== null) core.setModel(plan, settings);
  const runs = JSON.parse(core.takeText(core.exports.review_runs(plan))) as VideoRun[];
  const run = runs[request.run];
  if (!run) {
    await keySamples.return?.(undefined);
    say({ kind: 'part', part: null });
    return;
  }
  const total = runs.reduce((sum, videoRun) => sum + videoRun.frames, 0);
  say({ kind: 'progress', stage: 'looking', done: 0, total });
  const { fixed, hud } = await readKeys(decoding, plan, keySamples, firstBlock, share.lumaBytes);
  camera.start({ kind: 'start', setup, run: request.run, fixed, hud });
  const calls = await DetectorCalls.start(
    detector,
    fixed,
    Math.max(1, request.batch),
    request.modelUrl,
  );
  const tracker = new TrackerFeed(core, plan, request.run, total);
  const depth = detector.device === 'webgpu' ? GPU_CALLS_IN_FLIGHT : 1;
  const batches = new Batches(calls, tracker, depth);
  const end = runEnd(run, frameTimes.times, fps);
  const videoFrames = video.samples.samples(run.from, end)[Symbol.asyncIterator]();
  await readRun(videoFrames, decoding, { camera, share, tracker, batches });
  await batches.finish();
  const track = core.takeOutcome(core.exports.tracking_part(tracker.tracking));
  const watch = await camera.finish();
  frames.free();
  say({ kind: 'part', part: { setup, track, watch, fixed, device: detector.device } });
}
