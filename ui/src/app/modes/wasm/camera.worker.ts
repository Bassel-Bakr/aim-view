/// <reference lib="webworker" />
// The camera watch (src/camera.rs) and the HUD watch (src/hud.rs) in a worker of their own, beside the review worker:
// each frame's turn of the camera, whether KovaaK's countdown bar shows, and what the HUD reads (the kills, shots and
// hits of a run without a stats file, and a check on one with it). The review worker sends it, over a port between the
// two, every key frame's decoded Y plane in the fixed map's pass (the HUD watch finds its boxes from them), then every
// frame's Y plane and the rows of its RGB the countdown test reads; this worker makes the 720p luma with its own copy
// of the core (the same converter, so the same bytes) and sends each buffer back. In the review worker the watch held
// up the detector (av1: 76 frames a second with it there, 99 to 119 without it). The luma alone and the review
// worker's RGB rows, instead of every frame converted to RGB and YUV here, keep it light.
import { Core, CoreBlock } from './core';
import { CameraReply, CameraStart, CameraTask, WatchOpen, WatchParts } from './review-messages';

const W = 1280;
const H = 720;

/**
 * The watches, once opened: the core, its converter, the HUD watch, the camera watch (0 until started, after the key
 * frames), and the buffers they read and fill.
 */
interface Watch {
  core: Core;
  converter: number;
  hud: number;
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
        if (task.kind === 'open') {
          watch = await open(task);
          return;
        }
        const w = watch;
        if (!w) throw new Error('The camera worker was not opened');
        if (task.kind === 'key') {
          readKey(w, task.frame);
          say({ kind: 'free', frame: task.frame }, [task.frame]);
        } else if (task.kind === 'start') {
          start(w, task);
        } else if (task.kind === 'frame') {
          read(w, task.frame);
          say({ kind: 'free', frame: task.frame }, [task.frame]);
        } else {
          say({ kind: 'part', part: finish(w) });
        }
      })
      .catch((err: unknown) =>
        say({ kind: 'error', error: err instanceof Error ? err.message : String(err) }),
      );
  };
}

async function open(t: WatchOpen): Promise<Watch> {
  const core = await Core.load(t.coreUrl);
  const rows = core.x.camera_rgb_rows();
  return {
    core,
    converter: core.x.converter_new(t.width, t.height, t.matrix, t.full),
    hud: core.x.hud_new(t.width, t.height, t.full),
    camera: 0,
    source: core.reserve(t.width * t.height),
    luma: core.reserve(W * H),
    rgb: core.reserve(W * H * 3),
    rowsAt: (rows & 0xffff) * W * 3,
  };
}

/** A key frame's Y plane, to the HUD watch. */
function readKey(w: Watch, frame: ArrayBuffer): void {
  w.core.bytes(w.source).set(new Uint8Array(frame, 0, w.source.len));
  w.core.x.hud_add_key(w.hud, w.source.ptr, w.source.len);
}

/** The camera watch, from the fixed map and the excluded areas; both watches skip the frames before the review's first. */
function start(w: Watch, t: CameraStart): void {
  const fixed = w.core.reserve(W * H);
  w.core.bytes(fixed).set(t.fixed);
  w.camera = w.core.camera(t.areas, fixed.ptr);
  w.core.free(fixed);
  if (t.skip) {
    w.core.x.camera_skip(w.camera, t.skip);
    w.core.x.hud_skip(w.hud, t.skip);
  }
}

/** One frame: the HUD watch reads its Y plane; the camera watch, its 720p luma (ffmpeg's pixels) and countdown rows. */
function read(w: Watch, frame: ArrayBuffer): void {
  if (!w.camera) throw new Error('The camera watch was not started');
  const y = w.source.len;
  w.core.bytes(w.source).set(new Uint8Array(frame, 0, y));
  w.core.bytes(w.rgb).set(new Uint8Array(frame, y), w.rowsAt);
  w.core.x.hud_add(w.hud, w.source.ptr, y);
  w.core.x.converter_luma(w.converter, w.source.ptr, y, w.luma.ptr);
  w.core.x.camera_add(w.camera, w.luma.ptr, w.rgb.ptr);
}

/** The watches' parts of the run (the page joins the runs' parts and works out the readings); the watches are done. */
function finish(w: Watch): WatchParts {
  if (!w.camera) throw new Error('The camera watch was not started');
  w.core.x.converter_free(w.converter);
  return {
    camera: w.core.takeText(w.core.x.camera_part(w.camera)),
    hud: w.core.takeText(w.core.x.hud_part(w.hud)),
  };
}
