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
    const m = this.focus.selected();
    const r = this.report();
    const path = m && r.paths[String(m.kill_number)];
    return m && path ? speedChart(m, path, r.fps, this.size(), this.smooth()) : null;
  });
  /** The flick's length, in milliseconds: the slider's range. */
  protected readonly flickMs = computed(() => {
    const m = this.focus.selected();
    return m ? Math.round(1000 * m.total) : 0;
  });
  protected readonly title = computed(() => {
    const m = this.focus.selected();
    return m
      ? `Kill ${m.kill_number}: ${m.D0.toFixed(1)}° ${arrow(m.direction_deg)}, ${Math.round(1000 * m.total)} ms`
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
    const stop = this.playback.onFrame((t) => this.moveHead(t));
    const resize = new ResizeObserver(([entry]) => {
      const { width, height } = entry.contentRect;
      if (width && height) this.size.set({ width, height });
    });
    resize.observe(this.svg().nativeElement);
    this.destroyRef.onDestroy(() => {
      stop();
      resize.disconnect();
    });
  }

  private moveHead(t: number): void {
    const m = this.model();
    const head = this.head()?.nativeElement;
    if (!m || !head) return;
    const frame = Math.min(m.lastFrame, Math.max(m.firstFrame, t * m.fps));
    const x = String(xOf(m, frame));
    head.setAttribute('x1', x);
    head.setAttribute('x2', x);
    if (this.playback.paused()) {
      const ms = Math.round((1000 * (frame - m.firstFrame)) / m.fps);
      const svg = this.svg().nativeElement;
      svg.setAttribute('aria-valuenow', String(ms));
      svg.setAttribute('aria-valuetext', `${ms} ms into the flick`);
    }
  }

  /** Left and Right step a frame; Home and End go to the flick's start and its kill. */
  protected stepWithKeys(e: KeyboardEvent): void {
    const m = this.model();
    if (!m) return;
    const frame = Math.round(this.playback.time * m.fps);
    const to: Record<string, number> = {
      ArrowRight: frame + 1,
      ArrowUp: frame + 1,
      ArrowLeft: frame - 1,
      ArrowDown: frame - 1,
      Home: m.firstFrame,
      End: m.lastFrame,
    };
    if (!(e.key in to)) return;
    e.preventDefault();
    this.playback.pause();
    this.playback.seek(Math.min(m.lastFrame, Math.max(m.firstFrame, to[e.key])) / m.fps);
  }

  /** The mouse's x across the chart, in the chart's own pixels. */
  private chartX(e: MouseEvent, m: SpeedChartModel): number {
    const r = (e.currentTarget as Element).getBoundingClientRect();
    return ((e.clientX - r.left) * m.size.width) / r.width;
  }

  protected showPoint(e: PointerEvent): void {
    const m = this.model();
    const marker = this.marker()?.nativeElement;
    const tip = this.tip()?.nativeElement;
    if (!m || !marker || !tip || !m.data.length) return;
    const x = this.chartX(e, m);
    const [f, v] = m.data.reduce((a, b) =>
      Math.abs(xOf(m, b[0]) - x) < Math.abs(xOf(m, a[0]) - x) ? b : a,
    );
    marker.setAttribute('cx', String(xOf(m, f)));
    marker.setAttribute('cy', String(yOf(m, v)));
    marker.setAttribute('visibility', 'visible');
    const ms = Math.round((1000 * (f - m.firstFrame)) / m.fps);
    tip.textContent = `${ms} ms · ${Math.round(v)} °/s${this.smooth() ? ' (smoothed)' : ''}`;
    tip.hidden = false;
    tip.style.left = `${(xOf(m, f) / m.size.width) * 100}%`;
  }

  protected hidePoint(): void {
    this.marker()?.nativeElement.setAttribute('visibility', 'hidden');
    const tip = this.tip()?.nativeElement;
    if (tip) tip.hidden = true;
  }

  protected goToPoint(e: MouseEvent): void {
    const m = this.model();
    if (!m) return;
    this.playback.pause();
    this.playback.seek(frameAt(m, this.chartX(e, m)) / m.fps);
  }

  protected toggleSmooth(): void {
    this.smooth.update((on) => !on);
    localStorage.setItem(SMOOTH_KEY, this.smooth() ? '1' : '0');
  }
}
