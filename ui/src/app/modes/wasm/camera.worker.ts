/// <reference lib="webworker" />
/**
 * A run's watches (the core's review session: src/session.rs, `RunWatching`) in a worker of their
 * own, beside the review worker: each frame's turn of the camera, whether KovaaK's countdown bar
 * shows, and what the HUD reads (the kills, shots and hits of a run without a stats file, and a
 * check on one with it). The review worker sends it, over a port between the two, the review's
 * setup and what the key frames gave, then every frame the run reads: its decoded Y plane and the
 * rows of its RGB the countdown test reads. This worker feeds them to its own copy of the core (the
 * same converter makes the 720p luma, so the same bytes) and sends each buffer back. In the review
 * worker the watches held up the detector (av1: 76 frames a second with them there, 99 to 119
 * without them). In: the port (the first message), then `CameraTask`s on it. Out: `CameraReply`s
 * on the port; at the end, the watches' part of the run as JSON.
 */
import { Core, CoreBlock, FRAME_PIXELS } from './core';
import { CameraReply, CameraTask, WatchStart } from './review-messages';

/**
 * The run's watches, once started: the core, the session's watches, and the block each frame is
 * read into.
 */
interface Watch {
  /** This worker's copy of the core. */
  core: Core;
  /** The core's handle of the run's watches (`review_watching`). */
  watching: number;
  /** The core memory each frame is copied into, made for the first frame; null until then. */
  frame: CoreBlock | null;
}

addEventListener('message', (event: MessageEvent<MessagePort>) => serve(event.data));

/** Takes the review worker's tasks from the port, one at a time and in order. */
function serve(port: MessagePort): void {
  const say = (reply: CameraReply, transfer: Transferable[] = []) =>
    port.postMessage(reply, transfer);
  let core: Promise<Core> | null = null;
  let watch: Watch | null = null;
  let queue: Promise<void> = Promise.resolve();
  port.onmessage = (event: MessageEvent<CameraTask>) => {
    const task = event.data;
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
        const started = watch;
        if (!started) throw new Error('The camera worker was not started');
        if (task.kind === 'frame') {
          read(started, task.frame);
          say({ kind: 'free', frame: task.frame }, [task.frame]);
        } else {
          say({
            kind: 'part',
            part: started.core.takeOutcome(started.core.exports.watching_part(started.watching)),
          });
        }
      })
      .catch((error: unknown) =>
        say({ kind: 'error', error: error instanceof Error ? error.message : String(error) }),
      );
  };
}

/**
 * The run's watches, from the review's setup and what the key frames gave (the fixed map and the
 * HUD's boxes). Throws when the core cannot read the HUD's boxes.
 */
function start(core: Core, task: WatchStart): Watch {
  const review = core.review(task.setup);
  const fixed = core.reserve(FRAME_PIXELS);
  core.bytes(fixed).set(task.fixed);
  const watching = core.textIn(task.hud, (ptr, len) =>
    core.exports.review_watching(review, task.run, fixed.ptr, ptr, len),
  );
  core.free(fixed);
  core.exports.review_free(review);
  if (!watching) throw new Error("The HUD's boxes from the key frames could not be read");
  return { core, watching, frame: null };
}

/** One frame: its Y plane and countdown rows, to the watches. */
function read(watch: Watch, frame: ArrayBuffer): void {
  watch.frame ??= watch.core.reserve(frame.byteLength);
  watch.core.bytes(watch.frame).set(new Uint8Array(frame));
  watch.core.exports.watching_frame(watch.watching, watch.frame.ptr, watch.frame.len);
}
