import { CameraReply, CameraStart, CameraTask, WatchOpen, WatchParts } from './review-messages';

/** Frames sent to the camera worker and not read yet: past this many, the review waits for one to come back. */
const IN_FLIGHT = 4;

/**
 * The review worker's side of the camera worker (camera.worker.ts): it is opened, then each key frame and each frame
 * goes to it in a buffer of its own (one size for both: take()), which comes back once read, and at the end its
 * watches' parts of the run come back. If it fails, the next call says why.
 */
export class CameraLink {
  private readonly free: ArrayBuffer[] = [];
  private made = 0;
  private wake: (() => void) | null = null;
  private failure: Error | null = null;
  private done: ((parts: WatchParts) => void) | null = null;
  private failed: ((error: Error) => void) | null = null;

  constructor(private readonly port: MessagePort) {
    port.onmessage = (e: MessageEvent<CameraReply>) => this.hear(e.data);
  }

  /** Opens the worker's watches, before the first key frame. */
  open(task: WatchOpen): void {
    this.post(task);
  }

  /** Starts the camera watch, after the last key frame. */
  start(task: CameraStart): void {
    this.post(task);
  }

  /** A buffer for the next frame, once one is free. */
  async take(size: number): Promise<ArrayBuffer> {
    while (!this.free.length && this.made >= IN_FLIGHT) {
      this.check();
      await new Promise<void>((r) => (this.wake = r));
    }
    this.check();
    const reused = this.free.pop();
    if (reused) return reused;
    this.made++;
    return new ArrayBuffer(size);
  }

  /** Sends a key frame's Y plane, its buffer from take(). */
  sendKey(frame: ArrayBuffer): void {
    this.post({ kind: 'key', frame }, [frame]);
  }

  /** Sends a frame, its buffer from take(). */
  send(frame: ArrayBuffer): void {
    this.post({ kind: 'frame', frame }, [frame]);
  }

  /** The watches' parts of the run (camera_part's and hud_part's JSON), once the camera worker has read every frame. */
  finish(): Promise<WatchParts> {
    return new Promise((resolve, reject) => {
      if (this.failure) return reject(this.failure);
      this.done = resolve;
      this.failed = reject;
      this.post({ kind: 'finish' });
    });
  }

  private check(): void {
    if (this.failure) throw this.failure;
  }

  private hear(m: CameraReply): void {
    if (m.kind === 'free') this.free.push(m.frame);
    else if (m.kind === 'part') this.done?.(m.part);
    else {
      this.failure = new Error(`The camera watch failed: ${m.error}`);
      this.failed?.(this.failure);
    }
    const wake = this.wake;
    this.wake = null;
    wake?.();
  }

  private post(task: CameraTask, transfer: Transferable[] = []): void {
    this.port.postMessage(task, transfer);
  }
}
