import { HistoryModel } from './progress-chart-model';

/** The chart's drawing: its colors, font and sizes, from the tokens (themes/progress-chart.scss). */
export interface HistoryStyle {
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

export function readStyle(element: Element): HistoryStyle {
  const css = getComputedStyle(element);
  const token = (name: string) => css.getPropertyValue(name).trim();
  return {
    run: token('--progress-chart-run'),
    median: token('--progress-chart-median'),
    best: token('--progress-chart-best'),
    current: token('--progress-chart-this'),
    grid: token('--grid'),
    label: token('--text-muted'),
    surface: token('--surface-1'),
    font: token('--progress-chart-font'),
    dot: Number(token('--progress-chart-dot')),
    currentDot: Number(token('--progress-chart-current')),
    ring: Number(token('--progress-chart-ring')),
    reach: Number(token('--progress-chart-reach')),
    line: Number(token('--progress-chart-line')),
    dash: Number(token('--progress-chart-dash')),
  };
}

/** Lines on the canvas sit on a pixel's center, so they stay sharp. */
const HALF_PIXEL = 0.5;
/** The scores' gap from the chart's left edge, and the dates' gap from its bottom, in pixels. */
const SCORE_GAP = 6;
const DATE_GAP = 2;
const FULL_CIRCLE = 2 * Math.PI;

/** The grid lines with their scores at the left, and the dates along the bottom. */
function drawAxes(
  context: CanvasRenderingContext2D,
  model: HistoryModel,
  style: HistoryStyle,
): void {
  context.font = style.font;
  context.lineWidth = 1;
  context.fillStyle = style.label;
  context.textBaseline = 'middle';
  context.textAlign = 'right';
  for (const tick of model.grid) {
    context.strokeStyle = style.grid;
    context.beginPath();
    context.moveTo(model.left, Math.round(tick.at) + HALF_PIXEL);
    context.lineTo(model.right, Math.round(tick.at) + HALF_PIXEL);
    context.stroke();
    context.fillText(tick.label, model.left - SCORE_GAP, tick.at);
  }
  context.textAlign = 'center';
  context.textBaseline = 'bottom';
  for (const date of model.dates) {
    context.fillText(date.label, date.at, model.size.height - DATE_GAP);
  }
}

/** The personal best's level, dashed across the chart. */
function drawBestLevel(
  context: CanvasRenderingContext2D,
  model: HistoryModel,
  style: HistoryStyle,
): void {
  context.strokeStyle = style.best;
  context.setLineDash([style.dash, style.dash]);
  context.beginPath();
  context.moveTo(model.left, model.best.y);
  context.lineTo(model.right, model.best.y);
  context.stroke();
  context.setLineDash([]);
}

/** Every run's dot, then the median line through them. */
function drawRuns(
  context: CanvasRenderingContext2D,
  model: HistoryModel,
  style: HistoryStyle,
): void {
  context.fillStyle = style.run;
  for (const dot of model.dots) {
    context.beginPath();
    context.arc(dot.x, dot.y, style.dot, 0, FULL_CIRCLE);
    context.fill();
  }
  context.strokeStyle = style.median;
  context.lineWidth = style.line;
  context.lineJoin = 'round';
  context.beginPath();
  model.median.forEach((point, i) =>
    i ? context.lineTo(point.x, point.y) : context.moveTo(point.x, point.y),
  );
  context.stroke();
}

/** The personal best ringed, and this run's dot over everything. */
function drawMarks(
  context: CanvasRenderingContext2D,
  model: HistoryModel,
  style: HistoryStyle,
): void {
  context.strokeStyle = style.best;
  context.beginPath();
  context.arc(model.best.x, model.best.y, style.ring, 0, FULL_CIRCLE);
  context.stroke();
  if (!model.current) return;
  context.fillStyle = style.current;
  context.strokeStyle = style.surface;
  context.beginPath();
  context.arc(model.current.x, model.current.y, style.currentDot, 0, FULL_CIRCLE);
  context.fill();
  context.stroke();
}

/** The chart on its canvas: the grid and dates, the best's level, the runs and their median, the best and this run. */
export function drawProgressChart(
  context: CanvasRenderingContext2D,
  model: HistoryModel,
  style: HistoryStyle,
): void {
  drawAxes(context, model, style);
  drawBestLevel(context, model, style);
  drawRuns(context, model, style);
  drawMarks(context, model, style);
}
