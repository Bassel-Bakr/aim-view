import {
  afterNextRender,
  afterRenderEffect,
  Component,
  computed,
  DestroyRef,
  ElementRef,
  inject,
  input,
  signal,
  untracked,
  viewChild,
} from '@angular/core';
import { isClickReport, Report, Tracks } from '../../api';
import { Button } from '../../controls/button';
import { Playback, RATES } from '../playback';
import { FlickFocus } from '../flick-focus';
import { clock } from '../track';
import { PathCost } from '../fastest-path/path-cost';
import { FaintCutoff } from '../../services/faint-cutoff';
import { drawFaint, FaintStyle, pointedTrack, readFaintStyle } from '../faint-cutoff/faint-overlay';
import {
  drawClick,
  drawPaths,
  drawTrack,
  OverlayStyle,
  PathFlags,
  readOverlayStyle,
} from './overlay';
import { listenQuietly } from '../../services/listen-quietly';

const OVERLAY_KEY = 'aimview-overlay';
const FASTEST_KEY = 'aimview-fastest';
const MINE_KEY = 'aimview-mine';
const RATE_LABELS: Record<number, string> = { 1: '1×', 0.5: '½×', 0.25: '¼×', 0.125: '⅛×' };
/** Keys typed into these are theirs. */
const FIELDS = 'input, textarea, select, dialog';
/** These also use Space and the arrows. */
const KEY_OWNERS = `${FIELDS}, [role=listbox], [role=slider]`;
/** Before a review gives the video's frame rate, Left and Right step a frame at this rate. */
const FPS_BEFORE_REVIEW = 60;

/**
 * Calls back with the time of each frame the video shows, through the video's own frame callback; where the browser
 * has none, once per screen refresh. Returns the function that stops it.
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
 * Sizes the canvas's own pixels to its size on screen (in CSS pixels) at the screen's pixel ratio, so the overlay is
 * sharp. Returns the ratio.
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

/** Where the seek bar marks the run's moments, as shares of the video (0 to 100): kills, or a tracking run's deaths. */
export function markPositions(report: Report | null, duration: number): number[] {
  if (!report || !(duration > 0)) return [];
  const frames =
    report.mode === 'track'
      ? report.summary.switches.map(([start]) => start)
      : report.flicks.map((flick) => flick.kill_frame);
  return frames.map((frame) => (100 * frame) / report.fps / duration);
}

/**
 * The video with the review drawn over it, the seek bar and the playback controls, and the panels that work on the
 * video above it (projected with the `panel` attribute). The overlay, the clock and the seek bar follow every frame
 * through the video's frame callback, outside change detection. Full screen (the button, or F) fills the screen with
 * the video, its timeline and the controls, and puts the panels below them; where the browser refuses full screen, the
 * player fills the window the same way.
 */
@Component({
  selector: 'app-player',
  imports: [Button],
  templateUrl: './player.html',
  styleUrl: './player.scss',
  host: {
    '(document:keydown)': 'handleKeydown($event)',
    '(document:fullscreenchange)': 'followFullScreen()',
  },
})
export class Player {
  readonly src = input.required<string>();
  readonly report = input<Report | null>(null);
  readonly tracks = input<Tracks | null>(null);
  /** Something is edited on the video (the excluded areas, projected with the `screen` attribute): no overlay. */
  readonly editing = input(false);
  protected readonly playback = inject(Playback);
  protected readonly focus = inject(FlickFocus);
  private readonly paths = inject(PathCost);
  private readonly faint = inject(FaintCutoff);
  private readonly destroyRef = inject(DestroyRef);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  private readonly screenBox = viewChild.required<ElementRef<HTMLElement>>('screenBox');
  private readonly video = viewChild.required<ElementRef<HTMLVideoElement>>('video');
  private readonly canvas = viewChild.required<ElementRef<HTMLCanvasElement>>('overlay');
  private readonly clockText = viewChild.required<ElementRef<HTMLElement>>('clock');
  private readonly seekBar = viewChild.required<ElementRef<HTMLInputElement>>('seek');

  protected readonly rates = RATES;
  protected readonly rateLabels = RATE_LABELS;
  protected readonly aspect = signal<string | null>(null);
  protected readonly showOverlay = signal(localStorage.getItem(OVERLAY_KEY) !== '0');
  protected readonly showFastest = signal(localStorage.getItem(FASTEST_KEY) === '1');
  protected readonly showMine = signal(localStorage.getItem(MINE_KEY) !== '0');
  /** The browser shows the player full screen. */
  private readonly screen = signal(false);
  /** The browser refused full screen, so the player fills the window instead. */
  private readonly windowed = signal(false);
  /** The player fills the screen, or the window. */
  protected readonly full = computed(() => this.screen() || this.windowed());
  protected readonly clicking = computed(() => isClickReport(this.report()));
  protected readonly marks = computed(() => markPositions(this.report(), this.playback.duration()));
  /** The video's length, in whole seconds: the seek bar's end. */
  protected readonly end = computed(() =>
    clock(this.playback.duration() || 0).replace(/\.\d$/, ''),
  );
  private style: OverlayStyle | null = null;
  private faintStyle: FaintStyle | null = null;
  private seeking = false;

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
      untracked(() => this.draw(this.playback.time));
    });
  }

  /**
   * Starts following the video's frames (the overlay, the clock and the seek bar redraw on each one), the mouse over
   * it and the seek bar being dragged, these outside the template (listenQuietly): only a new track under the mouse
   * needs change detection, through its signal.
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
    this.destroyRef.onDestroy(() => {
      cancel();
      resize.disconnect();
      stop();
      this.playback.attach(null);
    });
  }

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
    this.style ??= readOverlayStyle(canvas);
    const frame = Math.round(seconds * report.fps);
    const scale = widthPx / report.geometry.W;
    this.drawReview(context, report, frame, scale, this.style);
    this.drawCutoff(context, frame, scale);
  }

  /** A clicking run's paths and its flick, or a tracking run's boxes (overlay.ts). */
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

  /** What the faint-target cut-off leaves out, dimmed, and the scores when asked for (faint-overlay.ts). */
  private drawCutoff(context: CanvasRenderingContext2D, frame: number, scale: number): void {
    const report = this.report();
    const all = this.faint.allTracks();
    const faintScores = this.faint.scores();
    if (!report || !all || !faintScores || !this.faint.has()) return;
    this.faintStyle ??= readFaintStyle(this.canvas().nativeElement);
    drawFaint(context, report, all, frame, scale, this.faintStyle, {
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

  private stopPointing(): void {
    if (this.faint.hover()) this.faint.hover.set(null);
  }

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

  protected showSeekedFrame(): void {
    this.playback.frame(this.video().nativeElement.currentTime);
  }

  protected startSeeking(): void {
    this.seeking = true;
  }

  protected stopSeeking(): void {
    this.seeking = false;
  }

  protected toggleOverlay(): void {
    this.showOverlay.update((on) => !on);
    localStorage.setItem(OVERLAY_KEY, this.showOverlay() ? '1' : '0');
  }

  protected toggleFastest(): void {
    this.showFastest.update((on) => !on);
    localStorage.setItem(FASTEST_KEY, this.showFastest() ? '1' : '0');
  }

  protected toggleMine(): void {
    this.showMine.update((on) => !on);
    localStorage.setItem(MINE_KEY, this.showMine() ? '1' : '0');
  }

  /** Fills the screen with the player, or leaves full screen. Where the browser refuses, the player fills the window. */
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
   * Full screen started or ended (the browser's own Escape ends it too). It starts on the video, not where the panels
   * below it were scrolled to last time.
   */
  protected followFullScreen(): void {
    const host = this.host.nativeElement;
    this.screen.set(document.fullscreenElement === host);
    if (this.screen()) host.scrollTop = 0;
  }

  /**
   * The player fills the window, laid out as in full screen. It goes into the page's top layer as a popover, which
   * no container of the page holds in (the main area measures its width, so it would hold a fixed layer in it).
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

  private leaveWindow(): void {
    const host = this.host.nativeElement;
    host.hidePopover();
    host.removeAttribute('popover');
    this.windowed.set(false);
  }

  /**
   * Space plays or pauses; Left and Right step one frame; Shift with Left or Right replays the previous or next flick
   * (a tracking run: goes to the previous or next bot's death); F goes full screen or leaves it, and so does Escape
   * where the browser leaves it to the page (always, when the player fills the window). Keys typed into a field, and Space and the arrows used by a list or a
   * slider, are theirs.
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
