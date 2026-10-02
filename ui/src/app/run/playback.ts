import { Injectable, signal } from '@angular/core';

/** Called with the time of the frame on screen, in seconds, once per video frame. */
export type FrameListener = (seconds: number) => void;

export const RATES = [1, 0.5, 0.25, 0.125];

/**
 * The video: playing, seeking and speed, and the frame on screen. The frame changes up to 120 times a second, so it is
 * not a signal: what follows it (the overlay, the timeline's playhead, the clock) registers a listener and writes to
 * its canvas or element directly, without change detection.
 */
@Injectable({ providedIn: 'root' })
export class Playback {
  readonly paused = signal(true);
  readonly rate = signal(0.25);
  readonly duration = signal(0);
  time = 0;
  private video: HTMLVideoElement | null = null;
  private stopAt: number | null = null;
  private readonly listeners = new Set<FrameListener>();

  /** A stretch is playing that stops by itself (playRange): one flick, replayed. */
  get replaying(): boolean {
    return this.stopAt !== null;
  }

  /** The player hands over its video element, and takes it back with null. */
  attach(video: HTMLVideoElement | null): void {
    this.video = video;
    this.time = 0;
    this.stopAt = null;
    if (video) video.playbackRate = this.rate();
  }

  onFrame(listener: FrameListener): () => void {
    this.listeners.add(listener);
    listener(this.time);
    return () => this.listeners.delete(listener);
  }

  /** The player calls this for each frame it shows, and after a seek. */
  frame(seconds: number): void {
    this.time = seconds;
    if (this.stopAt !== null && seconds >= this.stopAt) {
      this.video?.pause();
      this.stopAt = null;
    }
    for (const listener of this.listeners) listener(seconds);
  }

  toggle(): void {
    const v = this.video;
    if (!v) return;
    this.stopAt = null;
    if (v.paused) v.play().catch(() => undefined);
    else v.pause();
  }

  pause(): void {
    this.stopAt = null;
    this.video?.pause();
  }

  seek(seconds: number): void {
    const v = this.video;
    if (!v) return;
    this.stopAt = null;
    v.currentTime = Math.max(0, Math.min(v.duration || 0, seconds));
  }

  /** Plays from one time until another, then pauses: one flick, replayed. */
  playRange(from: number, to: number): void {
    const v = this.video;
    if (!v) return;
    this.seek(from);
    this.stopAt = to;
    v.play().catch(() => undefined);
  }

  /** Frames on (or back with a negative count), paused. */
  step(frames: number, fps: number): void {
    this.video?.pause();
    this.seek(this.time + frames / fps);
  }

  setRate(rate: number): void {
    this.rate.set(rate);
    if (this.video) this.video.playbackRate = rate;
  }
}
