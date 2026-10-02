import { tv } from 'tailwind-variants/lite';

/** A flick's speed chart: its header, the chart and the hover tooltip. */
export const speedChartStyles = tv({
  slots: {
    head: 'flex flex-wrap items-center gap-x-5 gap-y-1',
    title: 'font-strong text-primary',
    label: 'text-xs text-muted',
    frame: 'relative mt-2',
    svg: 'block h-(--speed-chart-height) w-full cursor-crosshair',
    idle: 'mt-2 text-xs text-muted',
    tip: 'pointer-events-none absolute top-5 rounded-md bg-surface-3 px-4 py-1 text-xs whitespace-nowrap',
  },
});
