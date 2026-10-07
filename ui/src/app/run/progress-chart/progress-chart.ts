/**
 * The progress chart on the run page: the run's score against the scenario's past runs.
 *
 * In: the recording (its scenario and time stamp), the scenario's past runs from KovaaK's stats
 * files (ScoreHistory), the recordings in the list (Library) and the user's pointer.
 * Out: the chart drawn on a canvas (progress-chart-drawing.ts), a tip on the run under the pointer,
 * and a click that opens a run's recording (Library `selectedId`).
 */

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
 * Every past score of the run's scenario, from KovaaK's stats files, over the days it was played:
 * each run a dot, the median of the runs around each as a line, the personal best ringed and this
 * run marked. Clicking a run opens its recording when the list has one; else its date and score
 * show under the chart.
 */
@Component({
  selector: 'app-progress-chart',
  // no chart when the history cannot be read (a review server without /api/history)
  host: { '[hidden]': '!!runs.error()' },
  templateUrl: './progress-chart.html',
  styleUrl: './progress-chart.scss',
})
export class ProgressChart {
  /** The recording on the page: its scenario's runs are charted, and its own run marked. */
  readonly recording = input.required<Recording>();
  /** The recordings in the list: a click on a run opens its recording there. */
  private readonly library = inject(Library);
  /** Stops the resize observer and the listeners when the chart goes. */
  private readonly destroyRef = inject(DestroyRef);
  /** Whether the user can give the stats folder here (the browser mode). */
  protected readonly needsFolder = inject(StatsFiles).chooseFolder !== null;
  /** The chart's box, which takes the pointer. */
  private readonly box = viewChild.required<ElementRef<HTMLElement>>('box');
  /** The canvas the chart is drawn on. */
  private readonly canvas = viewChild.required<ElementRef<HTMLCanvasElement>>('chart');
  /** The tip that describes the run under the pointer. */
  private readonly tip = viewChild.required<ElementRef<HTMLElement>>('tip');

  /** The scenario's past runs, oldest first, as a resource. */
  protected readonly runs = inject(ScoreHistory).runs(() => this.recording().scenario);
  /** The canvas's size on screen, in CSS pixels, kept by a resize observer. */
  private readonly size = signal<HistorySize>({ width: 0, height: 0 });
  /** The chart's layout; null with no runs or before the canvas has a size. */
  protected readonly model = computed<HistoryModel | null>(() => {
    const runs = this.runs.hasValue() ? this.runs.value() : undefined;
    const size = this.size();
    return runs?.length && size.width > 0
      ? progressChart(runs, this.recording().stamp, size)
      : null;
  });
  /** How many past runs there are; 0 while they load. */
  protected readonly count = computed(() => (this.runs.hasValue() ? this.runs.value()?.length : 0));
  /** The median line's legend. */
  protected readonly legend = `Median of ${MEDIAN_RUNS} runs`;
  /**
   * What the chart says when the scenario has no stats files: where to give them, in browser mode.
   */
  protected readonly emptyText = this.needsFolder
    ? "No stats files of this scenario yet: give KovaaK's stats folder with Stats folder at the top."
    : "No stats files of this scenario in KovaaK's stats folder.";
  /**
   * The run clicked that has no recording: its date and score (cleared when another recording
   * opens).
   */
  protected readonly picked = linkedSignal<Recording, string | null>({
    source: this.recording,
    computation: () => null,
  });
  /** The chart's colors and sizes, read from the tokens on the first draw. */
  private style: HistoryStyle | null = null;

  /**
   * Starts watching the canvas's size and the pointer after the first render; redraws on change.
   */
  constructor() {
    afterNextRender(() => this.follow());
    afterRenderEffect(() => {
      this.model();
      untracked(() => this.draw());
    });
  }

  /** The chart follows its canvas's size; the pointer shows a run's tip, and a click opens it. */
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

  /** Sizes the canvas at the screen's pixel ratio and draws the chart; nothing without a layout. */
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

  /** The run under the pointer, within the style's reach; null with none. */
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

  /**
   * Describes the run under the pointer in the tip, says whether a click opens it, and marks the
   * box (`data-over`) while a run is under the pointer; hides the tip otherwise.
   */
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

  /**
   * A click on a run opens its recording; a run with no recording in the list shows its date and
   * score under the chart instead.
   */
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
