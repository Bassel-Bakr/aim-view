/// <reference lib="webworker" />
// The camera watch (src/camera.rs) in a worker of its own, beside the review worker: each frame's turn of the camera and
// whether KovaaK's countdown bar shows. The review worker sends it every frame as decoded, over a port between the two.
// It makes the frame's 720p pixels with its own copy of the core (the same converter, so the same bytes) and sends the
// buffer back. In the review worker the watch held up the detector (av1: 76 frames a second with it there, 99 to 119
// without it).
import { Core, CoreBlock } from './core';
import { CameraReply, CameraStart, CameraTask, VideoReadings } from './review-messages';

const W = 1280;
const H = 720;

/** The watch, once started: the core, its converter and camera, and the buffers they read and fill. */
interface Watch {
  core: Core;
  converter: number;
  camera: number;
  source: CoreBlock;
  yuv720: CoreBlock;
  rgb: CoreBlock;
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
          read(w, task.yuv);
          say({ kind: 'free', yuv: task.yuv }, [task.yuv]);
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
  return {
    core,
    converter: core.x.converter_new(t.width, t.height, t.matrix, t.full),
    camera,
    source: core.reserve((t.width * t.height * 3) / 2),
    yuv720: core.reserve((W * H * 3) / 2),
    rgb: core.reserve(W * H * 3),
  };
}

/** One frame: its 720p RGB and YUV (ffmpeg's pixels), then the watch. */
function read(w: Watch, yuv: ArrayBuffer): void {
  w.core.bytes(w.source).set(new Uint8Array(yuv));
  w.core.x.converter_rgb24(w.converter, w.source.ptr, w.source.len, w.rgb.ptr);
  w.core.x.converter_yuv420p(w.converter, w.source.ptr, w.source.len, w.yuv720.ptr);
  w.core.x.camera_add(w.camera, w.yuv720.ptr, w.rgb.ptr);
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
