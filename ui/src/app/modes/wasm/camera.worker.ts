/// <reference lib="webworker" />
// A run's watches (the core's review session: src/session.rs, `RunWatching`) in a worker of their own, beside the
// review worker: each frame's turn of the camera, whether KovaaK's countdown bar shows, and what the HUD reads (the
// kills, shots and hits of a run without a stats file, and a check on one with it). The review worker sends it, over a
// port between the two, the review's setup and what the key frames gave, then every frame the run reads: its decoded Y
// plane and the rows of its RGB the countdown test reads. This worker feeds them to its own copy of the core (the same
// converter makes the 720p luma, so the same bytes) and sends each buffer back. In the review worker the watches held
// up the detector (av1: 76 frames a second with them there, 99 to 119 without them).
import { Core, CoreBlock } from './core';
import { CameraReply, CameraTask, WatchStart } from './review-messages';

const W = 1280;
const H = 720;

/** The run's watches, once started: the core, the session's watches, and the block each frame is read into. */
interface Watch {
  core: Core;
  watching: number;
  frame: CoreBlock | null;
}

addEventListener('message', (e: MessageEvent<MessagePort>) => serve(e.data));

/** Takes the review worker's tasks from the port, one at a time and in order. */
function serve(port: MessagePort): void {
  const say = (m: CameraReply, transfer: Transferable[] = []) => port.postMessage(m, transfer);
  let core: Promise<Core> | null = null;
  let watch: Watch | null = null;
  let queue: Promise<void> = Promise.resolve();
  port.onmessage = (e: MessageEvent<CameraTask>) => {
    const task = e.data;
    // the core loads as soon as the worker opens, while the review worker reads the key frames
    if (task.kind === 'open') core = Core.load(task.coreUrl);
    queue = queue
      .then(async () => {
        if (task.kind === 'open') return;
        if (task.kind === 'start') {
          if (!core) throw new Error('The camera worker was not opened');
          watch = start(await core, task);
          return;
        }
        const w = watch;
        if (!w) throw new Error('The camera worker was not started');
        if (task.kind === 'frame') {
          read(w, task.frame);
          say({ kind: 'free', frame: task.frame }, [task.frame]);
        } else {
          say({ kind: 'part', part: w.core.takeOutcome(w.core.x.watching_part(w.watching)) });
        }
      })
      .catch((err: unknown) =>
        say({ kind: 'error', error: err instanceof Error ? err.message : String(err) }),
      );
  };
}

/** The run's watches, from the review's setup and what the key frames gave. */
function start(core: Core, t: WatchStart): Watch {
  const review = core.review(t.setup);
  const fixed = core.reserve(W * H);
  core.bytes(fixed).set(t.fixed);
  const watching = core.textIn(t.hud, (ptr, len) =>
    core.x.review_watching(review, t.run, fixed.ptr, ptr, len),
  );
  core.free(fixed);
  core.x.review_free(review);
  if (!watching) throw new Error("The HUD's boxes from the key frames could not be read");
  return { core, watching, frame: null };
}

/** One frame: its Y plane and countdown rows, to the watches. */
function read(w: Watch, frame: ArrayBuffer): void {
  w.frame ??= w.core.reserve(frame.byteLength);
  w.core.bytes(w.frame).set(new Uint8Array(frame));
  w.core.x.watching_frame(w.watching, w.frame.ptr, w.frame.len);
}
