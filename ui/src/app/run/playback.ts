import { Service, signal } from '@angular/core';

/** Called with the time of the frame on screen, in seconds, once per video frame. */
export type FrameListener = (seconds: number) => void;

export const RATES = [1, 0.5, 0.25, 0.125];

/**
 * Plays a video at a speed, and makes it the speed a newly loaded video starts at: loading a video (another recording
 * in the same player) resets its speed to the default, which left the speed buttons saying one speed and the video
 * playing another.
 */
function applyRate(video: HTMLVideoElement, rate: number): void {
  video.defaultPlaybackRate = rate;
  video.playbackRate = rate;
}

/**
 * The video: playing, seeking and speed, and the frame on screen. The frame changes up to 120 times a second, so it is
 * not a signal: what follows it (the overlay, the timeline's playhead, the clock) registers a listener and writes to
 * its canvas or element directly, without change detection.
 */
@Service()
export class Playback {
  readonly paused = signal(true);
  readonly rate = signal(0.25);
  readonly duration = signal(0);
  time = 0;
  /**
   * Where the next video to load starts, in seconds (the labelling queue shows a frame from the run, not the
   * countdown); null: at its start.
   */
  startAt: number | null = null;
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
    if (video) applyRate(video, this.rate());
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
    const video = this.video;
    if (!video) return;
    this.stopAt = null;
    if (video.paused) video.play().catch(() => undefined);
    else video.pause();
  }

  pause(): void {
    this.stopAt = null;
    this.video?.pause();
  }

  seek(seconds: number): void {
    const video = this.video;
    if (!video) return;
    this.stopAt = null;
    video.currentTime = Math.max(0, Math.min(video.duration || 0, seconds));
  }

  /** Plays from one time until another, then pauses: one flick, replayed. */
  playRange(from: number, to: number): void {
    const video = this.video;
    if (!video) return;
    this.seek(from);
    this.stopAt = to;
    video.play().catch(() => undefined);
  }

  /** Frames on (or back with a negative count), paused. */
  step(frames: number, fps: number): void {
    this.video?.pause();
    this.seek(this.time + frames / fps);
  }

  setRate(rate: number): void {
    this.rate.set(rate);
    if (this.video) applyRate(this.video, rate);
  }

  /** The video's speed changed (the browser's own controls, a picture-in-picture window): the buttons follow it. */
  followRate(): void {
    if (this.video) this.rate.set(this.video.playbackRate);
  }
}
