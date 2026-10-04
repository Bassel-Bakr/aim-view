import { CameraReply, CameraTask, WatchOpen, WatchStart } from './review-messages';

/** Frames sent to the camera worker and not read yet: past this many, the review waits for one to come back. */
const IN_FLIGHT = 4;

/**
 * The review worker's side of the camera worker (camera.worker.ts): it is opened, started after the key frames, then
 * each frame goes to it in a buffer of its own (take()), which comes back once read, and at the end its watches' part
 * of the run comes back. If it fails, the next call says why.
 */
export class CameraLink {
  private readonly free: ArrayBuffer[] = [];
  private made = 0;
  private wake: (() => void) | null = null;
  private failure: Error | null = null;
  private done: ((part: string) => void) | null = null;
  private failed: ((error: Error) => void) | null = null;

  constructor(private readonly port: MessagePort) {
    port.onmessage = (event: MessageEvent<CameraReply>) => this.hear(event.data);
  }

  /** Opens the worker (its core loads), before the key frames. */
  open(task: WatchOpen): void {
    this.post(task);
  }

  /** Starts the run's watches, after the last key frame. */
  start(task: WatchStart): void {
    this.post(task);
  }

  /** A buffer for the next frame, once one is free. */
  async take(size: number): Promise<ArrayBuffer> {
    while (!this.free.length && this.made >= IN_FLIGHT) {
      this.check();
      await new Promise<void>((resolve) => (this.wake = resolve));
    }
    this.check();
    const reused = this.free.pop();
    if (reused) return reused;
    this.made++;
    return new ArrayBuffer(size);
  }

  /** Sends a frame, its buffer from take(). */
  send(frame: ArrayBuffer): void {
    this.post({ kind: 'frame', frame }, [frame]);
  }

  /** The watches' part of the run (watching_part's JSON), once the camera worker has read every frame. */
  finish(): Promise<string> {
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

  private hear(reply: CameraReply): void {
    if (reply.kind === 'free') this.free.push(reply.frame);
    else if (reply.kind === 'part') this.done?.(reply.part);
    else {
      this.failure = new Error(`The camera watch failed: ${reply.error}`);
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
