/**
 * The run page's video player.
 *
 * In: the recording's video address, the review's report and tracks (run.ts), the fastest-path
 * analysis (PathCost), the faint cut-off (FaintCutoff) and the user's keys and clicks.
 * Out: the video with the review drawn over it (overlay.ts, faint-overlay.ts), the seek bar with
 * the run's kills marked, and the playback controls; it drives Playback, which the rest of the page
 * follows.
 */

import {
  afterNextRender,
  afterRenderEffect,
  Component,
  computed,
  DestroyRef,
  ElementRef,
  inject,
  Injector,
  input,
  signal,
  untracked,
  viewChild,
} from '@angular/core';
import { isClickReport, Report, Tracks } from '../../api';
import { Playback, RATES } from '../playback';
import { FlickFocus } from '../flick-focus';
import { clock } from '../track';
import { PathCost } from '../fastest-path/path-cost';
import { FaintCutoff } from '../../services/faint-cutoff';
import { drawFaint, pointedTrack, readFaintStyle } from '../faint-cutoff/faint-overlay';
import {
  drawClick,
  drawPaths,
  drawTrack,
  OverlayStyle,
  PathFlags,
  readOverlayStyle,
} from './overlay';
import { listenQuietly } from '../../services/listen-quietly';
import { FloatingPlayer } from './floating-player';
import { MiniBar } from './mini-bar/mini-bar';
import { Theme } from '../../services/theme';

/** The local storage key that keeps "Show the tracked target" across visits ('0' is off). */
const OVERLAY_KEY = 'aimview-overlay';
/** The local storage key that keeps "Show the fastest path" across visits ('1' is on). */
const FASTEST_KEY = 'aimview-fastest';
/** The local storage key that keeps "Show my path" across visits ('0' is off). */
const MINE_KEY = 'aimview-mine';
/** Each playback speed's button text. */
const RATE_LABELS: Record<number, string> = { 1: '1×', 0.5: '½×', 0.25: '¼×', 0.125: '⅛×' };
/** Keys typed into these are theirs. */
const FIELDS = 'input, textarea, select, dialog';
/** These also use Space and the arrows. */
const KEY_OWNERS = `${FIELDS}, [role=listbox], [role=slider]`;
/** Before a review gives the video's frame rate, Left and Right step a frame at this rate. */
const FPS_BEFORE_REVIEW = 60;

/**
 * Calls back with the time of each frame the video shows, in seconds, through the video's own frame
 * callback; where the browser has none, once per screen refresh while the video plays. Returns the
 * function that stops it.
 */
export function everyFrame(video: HTMLVideoElement, shown: (seconds: number) => void): () => void {
  let handle = 0;
  const optional: Partial<HTMLVideoElement> = video;
  if (optional.requestVideoFrameCallback) {
    const tick: VideoFrameRequestCallback = (_now, meta) => {
      shown(meta.mediaTime);
      handle = video.requestVideoFrameCallback(tick);
    };
    handle = video.requestVideoFrameCallback(tick);
    return () => video.cancelVideoFrameCallback(handle);
  }
  const loop = () => {
    if (!video.paused) shown(video.currentTime);
    handle = requestAnimationFrame(loop);
  };
  handle = requestAnimationFrame(loop);
  return () => cancelAnimationFrame(handle);
}

/**
 * Sizes the canvas's own pixels to its size on screen (in CSS pixels) at the screen's pixel ratio,
 * so the overlay is sharp. Returns the ratio.
 */
function fitCanvas(canvas: HTMLCanvasElement, widthPx: number, heightPx: number): number {
  const pixelRatio = devicePixelRatio || 1;
  const width = Math.round(widthPx * pixelRatio);
  const height = Math.round(heightPx * pixelRatio);
  if (canvas.width !== width || canvas.height !== height) {
    canvas.width = width;
    canvas.height = height;
  }
  return pixelRatio;
}

/**
 * Where the seek bar marks the run's moments, as percents of the video's length (0 to 100): kills,
 * or a tracking run's deaths. `duration` is the video's length in seconds; no marks without a
 * report or a length.
 */
export function markPositions(report: Report | null, duration: number): number[] {
  if (!report || !(duration > 0)) return [];
  const frames =
    report.mode === 'track'
      ? report.summary.switches.map(([start]) => start)
      : report.flicks.map((flick) => flick.kill_frame);
  return frames.map((frame) => (100 * frame) / report.fps / duration);
}

/**
 * The video with the review drawn over it, the seek bar and the playback controls, and the panels
 * that work on the video above it (projected with the `panel` attribute). The overlay, the clock
 * and the seek bar follow every frame through the video's frame callback, outside change detection.
 * Full screen (the button, or F) fills the screen with the video, its timeline and the controls,
 * and puts the panels below them; where the browser refuses full screen, the player fills the
 * window the same way.
 */
@Component({
  selector: 'app-player',
  imports: [MiniBar],
  templateUrl: './player.html',
  styleUrl: './player.scss',
  host: {
    '(document:keydown)': 'handleKeydown($event)',
    '(document:fullscreenchange)': 'followFullScreen()',
  },
})
export class Player {
  /** The video's address. */
  readonly src = input.required<string>();
  /** The review's report, drawn over the video; null before a review. */
  readonly report = input<Report | null>(null);
  /**
   * The review's tracks, for a tracking run's boxes and a clicking run's paths; null until loaded.
   */
  readonly tracks = input<Tracks | null>(null);
  /**
   * Something is edited on the video (the excluded areas, projected with the `screen` attribute):
   * no overlay.
   */
  readonly editing = input(false);
  /** The video's state, shared with the rest of the run page. */
  protected readonly playback = inject(Playback);
  /**
   * The flick in focus, which Shift with Left or Right steps through; the template has its switch.
   */
  protected readonly focus = inject(FlickFocus);
  /** The fastest-path analysis, for the fastest and your-path overlays. */
  private readonly paths = inject(PathCost);
  /** The faint-target cut-off: what it leaves out is dimmed on the video. */
  private readonly faint = inject(FaintCutoff);
  /** Stops the frame callbacks and listeners when the player goes. */
  private readonly destroyRef = inject(DestroyRef);
  /** The player's own element, which goes full screen or fills the window. */
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  /** The box that keeps the video's place in the page while it floats. */
  private readonly frameBox = viewChild.required<ElementRef<HTMLElement>>('frameBox');
  /** The box with the video and its overlay, which floats. */
  private readonly screenBox = viewChild.required<ElementRef<HTMLElement>>('screenBox');
  /** The video element. */
  private readonly video = viewChild.required<ElementRef<HTMLVideoElement>>('video');
  /** The canvas over the video that the review is drawn on. */
  private readonly canvas = viewChild.required<ElementRef<HTMLCanvasElement>>('overlay');
  /** The clock left of the seek bar, written on every frame outside the template. */
  private readonly clockText = viewChild.required<ElementRef<HTMLElement>>('clock');
  /** The seek bar, moved on every frame outside the template. */
  private readonly seekBar = viewChild.required<ElementRef<HTMLInputElement>>('seek');

  /** The playback speeds offered, fastest first. */
  protected readonly rates = RATES;
  /** Each speed's button text. */
  protected readonly rateLabels = RATE_LABELS;
  /** The video's aspect ratio as CSS ("1920 / 1080"), once its size is known; null before. */
  protected readonly aspect = signal<string | null>(null);
  /** "Show the tracked target": the review drawn over the video. */
  protected readonly showOverlay = signal(localStorage.getItem(OVERLAY_KEY) !== '0');
  /** "Show the fastest path" on a clicking run (off unless turned on). */
  protected readonly showFastest = signal(localStorage.getItem(FASTEST_KEY) === '1');
  /** "Show my path" on a clicking run (on unless turned off). */
  protected readonly showMine = signal(localStorage.getItem(MINE_KEY) !== '0');
  /** The browser shows the player full screen. */
  private readonly screen = signal(false);
  /** The browser refused full screen, so the player fills the window instead. */
  private readonly windowed = signal(false);
  /** The player fills the screen, or the window. */
  protected readonly full = computed(() => this.screen() || this.windowed());
  /**
   * The video floating: docked in the page's corner while the player is scrolled away, or in its
   * own window.
   */
  protected readonly floating = new FloatingPlayer(
    this.playback,
    () => this.full(),
    inject(Injector),
  );
  /** Whether the report is a clicking run's, which adds the path switches and the follow switch. */
  protected readonly clicking = computed(() => isClickReport(this.report()));
  /** The seek bar's marks, as percents of the video's length (markPositions). */
  protected readonly marks = computed(() => markPositions(this.report(), this.playback.duration()));
  /** The video's length, in whole seconds: the seek bar's end. */
  protected readonly end = computed(() =>
    clock(this.playback.duration() || 0).replace(/\.\d$/, ''),
  );
  /** The color scheme: the colors below are read again when it changes. */
  private readonly theme = inject(Theme);
  /** The overlay's colors and fonts, read from the CSS variables on the first draw in each color scheme. */
  private readonly style = computed(() => {
    this.theme.scheme();
    return readOverlayStyle(this.canvas().nativeElement);
  });
  /** The cut-off overlay's colors and fonts, read from the CSS variables on its first draw in each color scheme. */
  private readonly faintStyle = computed(() => {
    this.theme.scheme();
    return readFaintStyle(this.canvas().nativeElement);
  });
  /** The seek bar is being dragged: the frames leave its value alone. */
  private seeking = false;

  /** Starts following the video after the first render, and redraws when what is drawn changes. */
  constructor() {
    afterNextRender(() => this.follow());
    afterRenderEffect(() => {
      this.report();
      this.tracks();
      this.showOverlay();
      this.showFastest();
      this.showMine();
      this.paths.analysis();
      this.editing();
      this.faint.dropped();
      this.faint.highlight();
      this.faint.showScores();
      this.faint.hover();
      this.style();
      this.faintStyle();
      untracked(() => this.draw(this.playback.time));
    });
  }

  /**
   * Starts following the video's frames (the overlay, the clock and the seek bar redraw on each
   * one), the mouse over it and the seek bar being dragged, these outside the template
   * (listenQuietly): only a new track under the mouse needs change detection, through its signal.
   */
  private follow(): void {
    const video = this.video().nativeElement;
    this.playback.attach(video);
    const stop = this.playback.onFrame((seconds) => this.draw(seconds));
    const cancel = everyFrame(video, (seconds) => this.playback.frame(seconds));
    const resize = new ResizeObserver(() => this.draw(this.playback.time));
    resize.observe(this.canvas().nativeElement);
    const box = this.screenBox().nativeElement;
    listenQuietly(box, 'mousemove', (event) => this.pointAt(event), this.destroyRef);
    listenQuietly(box, 'mouseleave', () => this.stopPointing(), this.destroyRef);
    const seek = this.seekBar().nativeElement;
    listenQuietly(seek, 'input', () => this.playback.seek(seek.valueAsNumber), this.destroyRef);
    this.floating.attach({
      frame: this.frameBox().nativeElement,
      screen: box,
      video,
      redraw: () => this.draw(this.playback.time),
    });
    this.destroyRef.onDestroy(() => {
      cancel();
      resize.disconnect();
      stop();
      this.playback.attach(null);
    });
  }

  /**
   * Shows the frame at `seconds` into the video: the clock, the seek bar (unless it is dragged),
   * and the overlay, cleared and drawn again at the canvas's size (nothing while editing or before
   * a review).
   */
  private draw(seconds: number): void {
    this.clockText().nativeElement.textContent = clock(seconds);
    if (!this.seeking) this.seekBar().nativeElement.value = String(seconds);
    const canvas = this.canvas().nativeElement;
    const widthPx = canvas.clientWidth;
    const heightPx = canvas.clientHeight;
    const pixelRatio = fitCanvas(canvas, widthPx, heightPx);
    const context = canvas.getContext('2d');
    if (!context) return;
    context.setTransform(pixelRatio, 0, 0, pixelRatio, 0, 0);
    context.clearRect(0, 0, widthPx, heightPx);
    const report = this.report();
    if (!report || this.editing()) return;
    const frame = Math.round(seconds * report.fps);
    const scale = widthPx / report.geometry.W;
    this.drawReview(context, report, frame, scale, this.style());
    this.drawCutoff(context, frame, scale);
  }

  /**
   * A clicking run's paths and its flick, or a tracking run's boxes (overlay.ts), at `frame`;
   * `scale` is CSS pixels per pixel of the review's frame.
   */
  private drawReview(
    context: CanvasRenderingContext2D,
    report: Report,
    frame: number,
    scale: number,
    style: OverlayStyle,
  ): void {
    const tracks = this.tracks();
    if (isClickReport(report)) {
      const paths = this.paths.analysis();
      const show: PathFlags = { fastest: this.showFastest(), mine: this.showMine() };
      if (tracks && paths && (show.fastest || show.mine)) {
        drawPaths(context, report, tracks, frame, scale, style, paths, show);
      }
      if (this.showOverlay()) drawClick(context, report, frame, scale, style);
    } else if (tracks && this.showOverlay()) {
      drawTrack(context, report, tracks, frame, scale, style);
    }
  }

  /**
   * What the faint-target cut-off leaves out, dimmed, and the scores when asked for
   * (faint-overlay.ts).
   */
  private drawCutoff(context: CanvasRenderingContext2D, frame: number, scale: number): void {
    const report = this.report();
    const all = this.faint.allTracks();
    const faintScores = this.faint.scores();
    if (!report || !all || !faintScores || !this.faint.has()) return;
    drawFaint(context, report, all, frame, scale, this.faintStyle(), {
      scores: faintScores.scores,
      dropped: this.faint.dropped(),
      highlight: this.faint.highlight(),
      showScores: this.faint.showScores(),
      hover: this.faint.hover(),
    });
  }

  /** The track under the mouse, with its scores, for the cut-off. */
  private pointAt(event: MouseEvent): void {
    const report = this.report();
    const all = this.faint.allTracks();
    const faintScores = this.faint.scores();
    if (!report || !all || !faintScores || !this.faint.has() || this.editing()) return;
    const box = this.canvas().nativeElement.getBoundingClientRect();
    const frame = Math.round(this.playback.time * report.fps);
    const scale = box.width / report.geometry.W;
    const hover = pointedTrack(
      report,
      all,
      frame,
      scale,
      event.clientX - box.left,
      event.clientY - box.top,
      faintScores.scores,
    );
    if ((hover?.text ?? null) !== (this.faint.hover()?.text ?? null)) this.faint.hover.set(hover);
  }

  /** The mouse left the video: no track is under it. */
  private stopPointing(): void {
    if (this.faint.hover()) this.faint.hover.set(null);
  }

  /**
   * The video's length and size are known: keeps its length and aspect ratio, seeks to a time the
   * page asked for before it loaded (Playback `startAt`), and shows the first frame.
   */
  protected loadedMetadata(): void {
    const video = this.video().nativeElement;
    this.playback.duration.set(video.duration);
    if (video.videoWidth && video.videoHeight)
      this.aspect.set(`${video.videoWidth} / ${video.videoHeight}`);
    if (this.playback.startAt !== null) {
      this.playback.seek(this.playback.startAt);
      this.playback.startAt = null;
    }
    this.playback.frame(video.currentTime);
  }

  /** A seek ended: the page follows the frame the video landed on. */
  protected showSeekedFrame(): void {
    this.playback.frame(this.video().nativeElement.currentTime);
  }

  /** The seek bar's drag started: the frames stop moving it. */
  protected startSeeking(): void {
    this.seeking = true;
  }

  /** The seek bar's drag ended: the frames move it again. */
  protected stopSeeking(): void {
    this.seeking = false;
  }

  /** Turns "Show the tracked target" on or off, and keeps the choice. */
  protected toggleOverlay(): void {
    this.showOverlay.update((on) => !on);
    localStorage.setItem(OVERLAY_KEY, this.showOverlay() ? '1' : '0');
  }

  /** Turns "Show the fastest path" on or off, and keeps the choice. */
  protected toggleFastest(): void {
    this.showFastest.update((on) => !on);
    localStorage.setItem(FASTEST_KEY, this.showFastest() ? '1' : '0');
  }

  /** Turns "Show my path" on or off, and keeps the choice. */
  protected toggleMine(): void {
    this.showMine.update((on) => !on);
    localStorage.setItem(MINE_KEY, this.showMine() ? '1' : '0');
  }

  /** Moves the video into the browser's picture-in-picture window, or brings it back. */
  protected toggleWindow(): void {
    void this.floating.toggleWindow();
  }

  /**
   * Fills the screen with the player, or leaves full screen. Where the browser refuses, the player
   * fills the window.
   */
  protected toggleFullScreen(): void {
    if (this.windowed()) {
      this.leaveWindow();
      return;
    }
    if (this.screen()) {
      void document.exitFullscreen().catch(() => undefined);
      return;
    }
    const host: Partial<HTMLElement> = this.host.nativeElement;
    const asked = host.requestFullscreen?.();
    if (asked) void asked.catch(() => this.fillWindow());
    else this.fillWindow();
  }

  /**
   * Full screen started or ended (the browser's own Escape ends it too). It starts on the video,
   * not where the panels below it were scrolled to last time.
   */
  protected followFullScreen(): void {
    const host = this.host.nativeElement;
    this.screen.set(document.fullscreenElement === host);
    if (this.screen()) host.scrollTop = 0;
  }

  /**
   * The player fills the window, laid out as in full screen. It goes into the page's top layer as a
   * popover, which no container of the page holds in (the main area measures its width, so it would
   * hold a fixed layer in it).
   */
  private fillWindow(): void {
    const host = this.host.nativeElement;
    const optional: Partial<HTMLElement> = host;
    if (!optional.showPopover) return;
    host.setAttribute('popover', 'manual');
    host.showPopover();
    host.scrollTop = 0;
    this.windowed.set(true);
  }

  /** The player stops filling the window and goes back in the page. */
  private leaveWindow(): void {
    const host = this.host.nativeElement;
    host.hidePopover();
    host.removeAttribute('popover');
    this.windowed.set(false);
  }

  /**
   * Space plays or pauses; Left and Right step one frame; Shift with Left or Right replays the
   * previous or next flick (a tracking run: goes to the previous or next bot's death); F goes full
   * screen or leaves it, and so does Escape where the browser leaves it to the page (always, when
   * the player fills the window). Keys typed into a field, and Space and the arrows used by a list
   * or a slider, are theirs.
   */
  protected handleKeydown(event: KeyboardEvent): void {
    if (event.ctrlKey || event.metaKey || event.altKey) return;
    const target = event.target instanceof Element ? event.target : null;
    if (target?.closest(FIELDS)) return;
    if (this.handleFullScreenKey(event)) return;
    if (target?.closest(KEY_OWNERS)) return;
    this.handlePlaybackKey(event);
  }

  /** F, and Escape: says whether the key was one of them. */
  private handleFullScreenKey(event: KeyboardEvent): boolean {
    if (event.key.toLowerCase() === 'f') {
      this.toggleFullScreen();
      return true;
    }
    if (event.key !== 'Escape') return false;
    // handled here: the excluded areas editor stays open
    if (this.full()) {
      event.preventDefault();
      this.toggleFullScreen();
    }
    return true;
  }

  /** Space, and Left and Right (with Shift, a flick or a death at a time). */
  private handlePlaybackKey(event: KeyboardEvent): void {
    if (event.key === ' ') {
      event.preventDefault();
      this.playback.toggle();
      return;
    }
    if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
    event.preventDefault();
    const forward = event.key === 'ArrowRight';
    if (event.shiftKey) this.jump(forward);
    else this.playback.step(forward ? 1 : -1, this.report()?.fps ?? FPS_BEFORE_REVIEW);
  }

  /** The previous or next flick, replayed; a tracking run: the previous or next bot's death. */
  private jump(forward: boolean): void {
    const report = this.report();
    if (!report) return;
    const frame = Math.round(this.playback.time * report.fps);
    if (report.mode === 'track') {
      const deaths = report.summary.switches.map(([start]) => start);
      const to = forward
        ? deaths.find((death) => death > frame)
        : [...deaths].reverse().find((death) => death < frame);
      if (to !== undefined) this.playback.seek(to / report.fps);
      return;
    }
    this.focus.step(forward);
  }
}
