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
  stampSeconds,
} from './progress-chart-model';

/** A stats file and a recording this many seconds apart or less are the same run. */
const SAME_RUN_S = 5;

/** The chart's drawing: its colors, font and sizes, from the tokens (themes/progress-chart.scss). */
interface HistoryStyle {
  run: string;
  median: string;
  best: string;
  current: string;
  grid: string;
  label: string;
  surface: string;
  font: string;
  dot: number;
  currentDot: number;
  ring: number;
  reach: number;
  line: number;
  dash: number;
}

function readStyle(el: Element): HistoryStyle {
  const css = getComputedStyle(el);
  const v = (name: string) => css.getPropertyValue(name).trim();
  return {
    run: v('--progress-chart-run'),
    median: v('--progress-chart-median'),
    best: v('--progress-chart-best'),
    current: v('--progress-chart-this'),
    grid: v('--grid'),
    label: v('--text-muted'),
    surface: v('--surface-1'),
    font: v('--progress-chart-font'),
    dot: Number(v('--progress-chart-dot')),
    currentDot: Number(v('--progress-chart-current')),
    ring: Number(v('--progress-chart-ring')),
    reach: Number(v('--progress-chart-reach')),
    line: Number(v('--progress-chart-line')),
    dash: Number(v('--progress-chart-dash')),
  };
}

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
    const move = (e: PointerEvent) => this.showTip(e);
    const leave = () => (this.tip().nativeElement.hidden = true);
    const down = (e: PointerEvent) => this.openRun(e);
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
    const m = this.model();
    const canvas = this.canvas().nativeElement;
    if (!m) return;
    const dpr = devicePixelRatio || 1;
    canvas.width = Math.round(m.size.width * dpr);
    canvas.height = Math.round(m.size.height * dpr);
    const c = canvas.getContext('2d');
    if (!c) return;
    c.setTransform(dpr, 0, 0, dpr, 0, 0);
    const s = (this.style ??= readStyle(canvas));
    c.font = s.font;
    c.lineWidth = 1;
    c.fillStyle = s.label;
    c.textBaseline = 'middle';
    c.textAlign = 'right';
    for (const g of m.grid) {
      c.strokeStyle = s.grid;
      c.beginPath();
      c.moveTo(m.left, Math.round(g.at) + 0.5);
      c.lineTo(m.right, Math.round(g.at) + 0.5);
      c.stroke();
      c.fillText(g.label, m.left - 6, g.at);
    }
    c.textAlign = 'center';
    c.textBaseline = 'bottom';
    for (const d of m.dates) c.fillText(d.label, d.at, m.size.height - 2);
    // the personal best's level
    c.strokeStyle = s.best;
    c.setLineDash([s.dash, s.dash]);
    c.beginPath();
    c.moveTo(m.left, m.best.y);
    c.lineTo(m.right, m.best.y);
    c.stroke();
    c.setLineDash([]);
    c.fillStyle = s.run;
    for (const d of m.dots) {
      c.beginPath();
      c.arc(d.x, d.y, s.dot, 0, 2 * Math.PI);
      c.fill();
    }
    c.strokeStyle = s.median;
    c.lineWidth = s.line;
    c.lineJoin = 'round';
    c.beginPath();
    m.median.forEach((p, i) => (i ? c.lineTo(p.x, p.y) : c.moveTo(p.x, p.y)));
    c.stroke();
    c.strokeStyle = s.best;
    c.beginPath();
    c.arc(m.best.x, m.best.y, s.ring, 0, 2 * Math.PI);
    c.stroke();
    if (m.current) {
      c.fillStyle = s.current;
      c.strokeStyle = s.surface;
      c.beginPath();
      c.arc(m.current.x, m.current.y, s.currentDot, 0, 2 * Math.PI);
      c.fill();
      c.stroke();
    }
  }

  /** The run under the pointer. */
  private dotAt(e: PointerEvent): HistoryDot | null {
    const m = this.model();
    if (!m) return null;
    const r = this.canvas().nativeElement.getBoundingClientRect();
    return dotNear(m, e.clientX - r.left, e.clientY - r.top, this.style?.reach ?? 0);
  }

  /** The recording of a run, when the list has one (same scenario, within five seconds). */
  private recordingOf(d: HistoryDot): Recording | null {
    const scenario = this.recording().scenario;
    return (
      this.library.all().find((r) => {
        const t = r.scenario === scenario ? stampSeconds(r.stamp) : null;
        return t !== null && Math.abs(t - d.seconds) <= SAME_RUN_S;
      }) ?? null
    );
  }

  /** A run in words: its date, time, score, kills and accuracy. */
  private describe(d: HistoryDot): string {
    const time = d.run.stamp.slice(11, 16).replace('.', ':');
    const parts = [`${d.date}, ${time}`, `score ${formatNumber(d.run.score)}`];
    if (d.run.kills !== null) parts.push(`${formatNumber(d.run.kills)} kills`);
    if (d.run.accuracy !== null) parts.push(`${formatPercent(d.run.accuracy)} accuracy`);
    return parts.join(' · ');
  }

  private showTip(e: PointerEvent): void {
    const tip = this.tip().nativeElement;
    const d = this.dotAt(e);
    this.box().nativeElement.toggleAttribute('data-over', !!d);
    tip.hidden = !d;
    if (!d) return;
    const open = this.recordingOf(d);
    const which = d === this.model()?.current ? ' · this run' : open ? ' · click to open it' : '';
    tip.textContent = this.describe(d) + which;
    const r = this.box().nativeElement.getBoundingClientRect();
    tip.style.left = `${Math.min(r.width - tip.offsetWidth, Math.max(0, d.x - tip.offsetWidth / 2))}px`;
  }

  private openRun(e: PointerEvent): void {
    const d = this.dotAt(e);
    if (!d) return;
    const open = this.recordingOf(d);
    if (open) {
      this.picked.set(null);
      this.library.selectedId.set(open.id);
    } else this.picked.set(`${this.describe(d)}: no recording of this run in the list`);
  }
}
