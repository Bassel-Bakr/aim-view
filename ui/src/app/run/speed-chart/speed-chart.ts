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
import { ClickReport } from '../../api';
import { arrow } from '../../format';
import { FlickFocus } from '../flick-focus';
import { listenQuietly } from '../../services/listen-quietly';
import { Playback } from '../playback';
import { ChartSize, frameAt, speedChart, SpeedChartModel, xOf, yOf } from './speed-chart-model';

const SMOOTH_KEY = 'aimview-smooth';

/**
 * The crosshair's speed through the flick in focus, with its moments (moving, flick done, on target, click). The
 * playhead follows the video and the hover marker the mouse, both outside change detection; a click goes there.
 */
@Component({
  selector: 'app-speed-chart',
  host: { '[class.idle]': '!model()' },
  templateUrl: './speed-chart.html',
  styleUrl: './speed-chart.scss',
})
export class SpeedChart {
  readonly report = input.required<ClickReport>();
  protected readonly focus = inject(FlickFocus);
  private readonly playback = inject(Playback);
  private readonly destroyRef = inject(DestroyRef);
  private readonly svg = viewChild.required<ElementRef<SVGSVGElement>>('svg');
  private readonly head = viewChild<ElementRef<SVGLineElement>>('head');
  private readonly marker = viewChild<ElementRef<SVGCircleElement>>('marker');
  private readonly tip = viewChild<ElementRef<HTMLElement>>('tip');

  protected readonly smooth = signal(localStorage.getItem(SMOOTH_KEY) !== '0');
  private readonly size = signal<ChartSize>({ width: 600, height: 150 });
  protected readonly model = computed<SpeedChartModel | null>(() => {
    const flick = this.focus.selected();
    const report = this.report();
    const path = flick && report.paths[String(flick.kill_number)];
    return flick && path ? speedChart(flick, path, report.fps, this.size(), this.smooth()) : null;
  });
  /** The flick's length, in milliseconds: the slider's range. */
  protected readonly flickMs = computed(() => {
    const flick = this.focus.selected();
    return flick ? Math.round(1000 * flick.total) : 0;
  });
  protected readonly title = computed(() => {
    const flick = this.focus.selected();
    return flick
      ? `Kill ${flick.kill_number}: ${flick.D0.toFixed(1)}° ${arrow(flick.direction_deg)}, ${Math.round(1000 * flick.total)} ms`
      : '';
  });

  constructor() {
    afterNextRender(() => this.follow());
    afterRenderEffect(() => {
      this.model();
      untracked(() => this.moveHead(this.playback.time));
    });
  }

  private follow(): void {
    const stop = this.playback.onFrame((seconds) => this.moveHead(seconds));
    const resize = new ResizeObserver(([entry]) => {
      const { width, height } = entry.contentRect;
      if (width && height) this.size.set({ width, height });
    });
    const svg = this.svg().nativeElement;
    resize.observe(svg);
    listenQuietly(svg, 'pointermove', (event) => this.showPoint(event), this.destroyRef);
    listenQuietly(svg, 'pointerleave', () => this.hidePoint(), this.destroyRef);
    this.destroyRef.onDestroy(() => {
      stop();
      resize.disconnect();
    });
  }

  private moveHead(seconds: number): void {
    const model = this.model();
    const head = this.head()?.nativeElement;
    if (!model || !head) return;
    const frame = Math.min(model.lastFrame, Math.max(model.firstFrame, seconds * model.fps));
    const x = String(xOf(model, frame));
    head.setAttribute('x1', x);
    head.setAttribute('x2', x);
    if (this.playback.paused()) {
      const ms = Math.round((1000 * (frame - model.firstFrame)) / model.fps);
      const svg = this.svg().nativeElement;
      svg.setAttribute('aria-valuenow', String(ms));
      svg.setAttribute('aria-valuetext', `${ms} ms into the flick`);
    }
  }

  /** Left and Right step a frame; Home and End go to the flick's start and its kill. */
  protected stepWithKeys(event: KeyboardEvent): void {
    const model = this.model();
    if (!model) return;
    const frame = Math.round(this.playback.time * model.fps);
    const to: Record<string, number> = {
      ArrowRight: frame + 1,
      ArrowUp: frame + 1,
      ArrowLeft: frame - 1,
      ArrowDown: frame - 1,
      Home: model.firstFrame,
      End: model.lastFrame,
    };
    if (!(event.key in to)) return;
    event.preventDefault();
    this.playback.pause();
    this.playback.seek(
      Math.min(model.lastFrame, Math.max(model.firstFrame, to[event.key])) / model.fps,
    );
  }

  /** The mouse's x across the chart, in the chart's own pixels. */
  private chartX(event: MouseEvent, model: SpeedChartModel): number {
    const bounds = (event.currentTarget as Element).getBoundingClientRect();
    return ((event.clientX - bounds.left) * model.size.width) / bounds.width;
  }

  private showPoint(event: PointerEvent): void {
    const model = this.model();
    const marker = this.marker()?.nativeElement;
    const tip = this.tip()?.nativeElement;
    if (!model || !marker || !tip || !model.data.length) return;
    const x = this.chartX(event, model);
    const [frame, speed] = model.data.reduce((a, b) =>
      Math.abs(xOf(model, b[0]) - x) < Math.abs(xOf(model, a[0]) - x) ? b : a,
    );
    marker.setAttribute('cx', String(xOf(model, frame)));
    marker.setAttribute('cy', String(yOf(model, speed)));
    marker.setAttribute('visibility', 'visible');
    const ms = Math.round((1000 * (frame - model.firstFrame)) / model.fps);
    tip.textContent = `${ms} ms · ${Math.round(speed)} °/s${this.smooth() ? ' (smoothed)' : ''}`;
    tip.hidden = false;
    tip.style.left = `${(xOf(model, frame) / model.size.width) * 100}%`;
  }

  private hidePoint(): void {
    this.marker()?.nativeElement.setAttribute('visibility', 'hidden');
    const tip = this.tip()?.nativeElement;
    if (tip) tip.hidden = true;
  }

  protected goToPoint(event: MouseEvent): void {
    const model = this.model();
    if (!model) return;
    this.playback.pause();
    this.playback.seek(frameAt(model, this.chartX(event, model)) / model.fps);
  }

  protected toggleSmooth(): void {
    this.smooth.update((on) => !on);
    localStorage.setItem(SMOOTH_KEY, this.smooth() ? '1' : '0');
  }
}
