/// <reference lib="webworker" />
// The camera watch (src/camera.rs) in a worker of its own, beside the review worker: each frame's turn of the camera and
// whether KovaaK's countdown bar shows. The review worker sends it, over a port between the two, every frame's decoded
// Y plane and the rows of its RGB the countdown test reads; this worker makes the 720p luma with its own copy of the
// core (the same converter, so the same bytes) and sends the buffer back. In the review worker the watch held up the
// detector (av1: 76 frames a second with it there, 99 to 119 without it). The luma alone and the review worker's RGB
// rows, instead of every frame converted to RGB and YUV here, keep it light.
import { Core, CoreBlock } from './core';
import { CameraReply, CameraStart, CameraTask, VideoReadings } from './review-messages';

const W = 1280;
const H = 720;

/** The watch, once started: the core, its converter and camera, and the buffers they read and fill. */
interface Watch {
  core: Core;
  converter: number;
  camera: number;
  /** The decoded Y plane, its 720p luma, and a 720p RGB frame of which only the countdown rows are filled. */
  source: CoreBlock;
  luma: CoreBlock;
  rgb: CoreBlock;
  /** Where the countdown rows go in it, in bytes. */
  rowsAt: number;
}

addEventListener('message', (e: MessageEvent<MessagePort>) => serve(e.data));

/** Takes the review worker's tasks from the port, one at a time and in order. */
function serve(port: MessagePort): void {
  const say = (m: CameraReply, transfer: Transferable[] = []) => port.postMessage(m, transfer);
  let watch: Watch | null = null;
  let queue: Promise<void> = Promise.resolve();
  port.onmessage = (e: MessageEvent<CameraTask>) => {
    const task = e.data;
    queue = queue
      .then(async () => {
        if (task.kind === 'start') {
          watch = await start(task);
          return;
        }
        const w = watch;
        if (!w) throw new Error('The camera watch was not started');
        if (task.kind === 'frame') {
          read(w, task.frame);
          say({ kind: 'free', frame: task.frame }, [task.frame]);
        } else {
          say({ kind: 'readings', readings: finish(w, task.frames) });
        }
      })
      .catch((err: unknown) =>
        say({ kind: 'error', error: err instanceof Error ? err.message : String(err) }),
      );
  };
}

async function start(t: CameraStart): Promise<Watch> {
  const core = await Core.load(t.coreUrl);
  const fixed = core.reserve(W * H);
  core.bytes(fixed).set(t.fixed);
  const camera = core.x.camera_new(fixed.ptr);
  core.free(fixed);
  const rows = core.x.camera_rgb_rows();
  return {
    core,
    converter: core.x.converter_new(t.width, t.height, t.matrix, t.full),
    camera,
    source: core.reserve(t.width * t.height),
    luma: core.reserve(W * H),
    rgb: core.reserve(W * H * 3),
    rowsAt: (rows & 0xffff) * W * 3,
  };
}

/** One frame: its 720p luma (ffmpeg's pixels) and its countdown rows, then the watch. */
function read(w: Watch, frame: ArrayBuffer): void {
  const y = w.source.len;
  w.core.bytes(w.source).set(new Uint8Array(frame, 0, y));
  w.core.bytes(w.rgb).set(new Uint8Array(frame, y), w.rowsAt);
  w.core.x.converter_luma(w.converter, w.source.ptr, y, w.luma.ptr);
  w.core.x.camera_add(w.camera, w.luma.ptr, w.rgb.ptr);
}

/** The readings, the tracks known; the watch is done. */
function finish(w: Watch, frames: string): VideoReadings {
  const bytes = new TextEncoder().encode(frames);
  const block = w.core.reserve(bytes.length);
  w.core.bytes(block).set(bytes);
  const text = w.core.takeText(w.core.x.camera_finish(w.camera, block.ptr, bytes.length));
  w.core.free(block);
  w.core.x.converter_free(w.converter);
  return JSON.parse(text) as VideoReadings;
}
