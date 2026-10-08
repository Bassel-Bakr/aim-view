/**
 * The run page's video controls, shared by every part that plays, seeks or follows the video. In:
 * the player's video element and its frame callbacks. Out: the player's buttons, the frame
 * listeners (overlay, timeline playhead, clock, FlickFocus) and the labelling queue's start time.
 */

import { Service, signal } from '@angular/core';

/** Called with the time of the frame on screen, in seconds, once per video frame. */
export type FrameListener = (seconds: number) => void;

/** The playback speeds the player offers, fastest first (1 is real time). */
export const RATES = [1, 0.5, 0.25, 0.125];

/**
 * Plays a video at a speed, and makes it the speed a newly loaded video starts at: loading a video
 * (another recording in the same player) resets its speed to the default, which left the speed
 * buttons saying one speed and the video playing another.
 */
function applyRate(video: HTMLVideoElement, rate: number): void {
  video.defaultPlaybackRate = rate;
  video.playbackRate = rate;
}

/**
 * The video: playing, seeking and speed, and the frame on screen. The frame changes up to 120 times
 * a second, so it is not a signal: what follows it (the overlay, the timeline's playhead, the
 * clock) registers a listener and writes to its canvas or element directly, without change
 * detection.
 */
@Service()
export class Playback {
  /** Whether the video is paused; the player sets it from the video's own events. */
  readonly paused = signal(true);
  /** The playback speed (one of RATES, or what the browser's own controls set). */
  readonly rate = signal(1);
  /** The video's length in seconds (0 before it loads); the player sets it. */
  readonly duration = signal(0);
  /** The time of the frame on screen, in seconds; not a signal, since it changes every frame. */
  time = 0;
  /**
   * Where the next video to load starts, in seconds (the labelling queue shows a frame from the
   * run, not the countdown); null: at its start.
   */
  startAt: number | null = null;
  /** The player's video element, or null while no player is attached. */
  private video: HTMLVideoElement | null = null;
  /** Where a replayed stretch pauses, in seconds; null when nothing is replaying. */
  private stopAt: number | null = null;
  /** Everything that follows the frame on screen. */
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

  /**
   * Calls the listener with each frame's time, starting at once with the frame on screen. Gives
   * back the function that stops it.
   */
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

  /** Plays a paused video or pauses a playing one; either ends a replay. */
  toggle(): void {
    const video = this.video;
    if (!video) return;
    this.stopAt = null;
    if (video.paused) video.play().catch(() => undefined);
    else video.pause();
  }

  /** Pauses the video and ends a replay. */
  pause(): void {
    this.stopAt = null;
    this.video?.pause();
  }

  /** Moves the video to a time in seconds, kept within the video, and ends a replay. */
  seek(seconds: number): void {
    const video = this.video;
    if (!video) return;
    this.stopAt = null;
    video.currentTime = Math.max(0, Math.min(video.duration || 0, seconds));
  }

  /** Plays from one time until another (seconds), then pauses: one flick, replayed. */
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

  /** Sets the playback speed (1 is real time), for this video and the next one loaded. */
  setRate(rate: number): void {
    this.rate.set(rate);
    if (this.video) applyRate(this.video, rate);
  }

  /**
   * The video's speed changed (the browser's own controls, a picture-in-picture window): the
   * buttons follow it.
   */
  followRate(): void {
    if (this.video) this.rate.set(this.video.playbackRate);
  }
}
