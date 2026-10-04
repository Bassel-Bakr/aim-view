import {
  afterNextRender,
  afterRenderEffect,
  Component,
  computed,
  DestroyRef,
  ElementRef,
  inject,
  input,
  linkedSignal,
  signal,
  untracked,
  viewChild,
} from '@angular/core';
import { Recording } from '../../api';
import { formatNumber, formatPercent } from '../../format';
import { ScoreHistory } from '../../platform/score-history';
import { StatsFiles } from '../../platform/stats-files';
import { Library } from '../../services/library';
import {
  dotNear,
  HistoryDot,
  HistoryModel,
  HistorySize,
  MEDIAN_RUNS,
  progressChart,
  SAME_RUN_S,
  stampSeconds,
} from './progress-chart-model';
import { drawProgressChart, HistoryStyle, readStyle } from './progress-chart-drawing';

/**
 * Every past score of the run's scenario, from KovaaK's stats files, over the days it was played: each run a dot, the
 * median of the runs around each as a line, the personal best ringed and this run marked. Clicking a run opens its
 * recording when the list has one; else its date and score show under the chart.
 */
@Component({
  selector: 'app-progress-chart',
  // no chart when the history cannot be read (a review server without /api/history)
  host: { '[hidden]': '!!runs.error()' },
  templateUrl: './progress-chart.html',
  styleUrl: './progress-chart.scss',
})
export class ProgressChart {
  readonly recording = input.required<Recording>();
  private readonly library = inject(Library);
  private readonly destroyRef = inject(DestroyRef);
  /** Whether the user can give the stats folder here (the browser mode). */
  protected readonly needsFolder = inject(StatsFiles).chooseFolder !== null;
  private readonly box = viewChild.required<ElementRef<HTMLElement>>('box');
  private readonly canvas = viewChild.required<ElementRef<HTMLCanvasElement>>('chart');
  private readonly tip = viewChild.required<ElementRef<HTMLElement>>('tip');

  protected readonly runs = inject(ScoreHistory).runs(() => this.recording().scenario);
  private readonly size = signal<HistorySize>({ width: 0, height: 0 });
  protected readonly model = computed<HistoryModel | null>(() => {
    const runs = this.runs.hasValue() ? this.runs.value() : undefined;
    const size = this.size();
    return runs?.length && size.width > 0
      ? progressChart(runs, this.recording().stamp, size)
      : null;
  });
  protected readonly count = computed(() => (this.runs.hasValue() ? this.runs.value()?.length : 0));
  protected readonly legend = `Median of ${MEDIAN_RUNS} runs`;
  protected readonly emptyText = this.needsFolder
    ? "No stats files of this scenario yet: give KovaaK's stats folder with Stats folder at the top."
    : "No stats files of this scenario in KovaaK's stats folder.";
  /** The run clicked that has no recording: its date and score (cleared when another recording opens). */
  protected readonly picked = linkedSignal<Recording, string | null>({
    source: this.recording,
    computation: () => null,
  });
  private style: HistoryStyle | null = null;

  constructor() {
    afterNextRender(() => this.follow());
    afterRenderEffect(() => {
      this.model();
      untracked(() => this.draw());
    });
  }

  private follow(): void {
    const box = this.box().nativeElement;
    const resize = new ResizeObserver(([entry]) => {
      const { width, height } = entry.contentRect;
      this.size.set({ width, height });
    });
    resize.observe(this.canvas().nativeElement);
    const move = (event: PointerEvent) => this.showTip(event);
    const leave = () => (this.tip().nativeElement.hidden = true);
    const down = (event: PointerEvent) => this.openRun(event);
    box.addEventListener('pointermove', move);
    box.addEventListener('pointerleave', leave);
    box.addEventListener('pointerdown', down);
    this.destroyRef.onDestroy(() => {
      resize.disconnect();
      box.removeEventListener('pointermove', move);
      box.removeEventListener('pointerleave', leave);
      box.removeEventListener('pointerdown', down);
    });
  }

  private draw(): void {
    const model = this.model();
    const canvas = this.canvas().nativeElement;
    if (!model) return;
    const pixelRatio = devicePixelRatio || 1;
    canvas.width = Math.round(model.size.width * pixelRatio);
    canvas.height = Math.round(model.size.height * pixelRatio);
    const context = canvas.getContext('2d');
    if (!context) return;
    context.setTransform(pixelRatio, 0, 0, pixelRatio, 0, 0);
    const style = (this.style ??= readStyle(canvas));
    drawProgressChart(context, model, style);
  }

  /** The run under the pointer. */
  private dotAt(event: PointerEvent): HistoryDot | null {
    const model = this.model();
    if (!model) return null;
    const bounds = this.canvas().nativeElement.getBoundingClientRect();
    return dotNear(
      model,
      event.clientX - bounds.left,
      event.clientY - bounds.top,
      this.style?.reach ?? 0,
    );
  }

  /** The recording of a run, when the list has one (same scenario, within five seconds). */
  private recordingOf(dot: HistoryDot): Recording | null {
    const scenario = this.recording().scenario;
    return (
      this.library.all().find((candidate) => {
        const seconds = candidate.scenario === scenario ? stampSeconds(candidate.stamp) : null;
        return seconds !== null && Math.abs(seconds - dot.seconds) <= SAME_RUN_S;
      }) ?? null
    );
  }

  /** A run in words: its date, time, score, kills and accuracy. */
  private describe(dot: HistoryDot): string {
    const time = dot.run.stamp.slice(11, 16).replace('.', ':');
    const parts = [`${dot.date}, ${time}`, `score ${formatNumber(dot.run.score)}`];
    if (dot.run.kills !== null) parts.push(`${formatNumber(dot.run.kills)} kills`);
    if (dot.run.accuracy !== null) parts.push(`${formatPercent(dot.run.accuracy)} accuracy`);
    return parts.join(' · ');
  }

  private showTip(event: PointerEvent): void {
    const tip = this.tip().nativeElement;
    const dot = this.dotAt(event);
    this.box().nativeElement.toggleAttribute('data-over', !!dot);
    tip.hidden = !dot;
    if (!dot) return;
    const open = this.recordingOf(dot);
    const which = dot === this.model()?.current ? ' · this run' : open ? ' · click to open it' : '';
    tip.textContent = this.describe(dot) + which;
    const bounds = this.box().nativeElement.getBoundingClientRect();
    tip.style.left = `${Math.min(bounds.width - tip.offsetWidth, Math.max(0, dot.x - tip.offsetWidth / 2))}px`;
  }

  private openRun(event: PointerEvent): void {
    const dot = this.dotAt(event);
    if (!dot) return;
    const open = this.recordingOf(dot);
    if (open) {
      this.picked.set(null);
      this.library.selectedId.set(open.id);
    } else this.picked.set(`${this.describe(dot)}: no recording of this run in the list`);
  }
}
