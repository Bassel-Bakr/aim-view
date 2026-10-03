import { CameraReply, CameraStart, CameraTask, VideoReadings } from './review-messages';

/** Frames sent to the camera worker and not read yet: past this many, the review waits for one to come back. */
const IN_FLIGHT = 4;

/**
 * The review worker's side of the camera worker (camera.worker.ts): each frame goes to it in a buffer of its own,
 * which comes back once read, and at the end its readings come back. If it fails, the next call says why.
 */
export class CameraLink {
  private readonly free: ArrayBuffer[] = [];
  private made = 0;
  private wake: (() => void) | null = null;
  private failure: Error | null = null;
  private done: ((readings: VideoReadings) => void) | null = null;
  private failed: ((error: Error) => void) | null = null;

  constructor(private readonly port: MessagePort) {
    port.onmessage = (e: MessageEvent<CameraReply>) => this.hear(e.data);
  }

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

  /** Sends a frame, its buffer from take(). */
  send(yuv: ArrayBuffer): void {
    this.post({ kind: 'frame', yuv }, [yuv]);
  }

  /** The readings, once the camera worker has read every frame. */
  finish(frames: string): Promise<VideoReadings> {
    return new Promise((resolve, reject) => {
      if (this.failure) return reject(this.failure);
      this.done = resolve;
      this.failed = reject;
      this.post({ kind: 'finish', frames });
    });
  }

  private check(): void {
    if (this.failure) throw this.failure;
  }

  private hear(m: CameraReply): void {
    if (m.kind === 'free') this.free.push(m.yuv);
    else if (m.kind === 'readings') this.done?.(m.readings);
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
