import { tv } from 'tailwind-variants/lite';

/** A tracking run's timeline: its legend, chart, playhead and tooltip. */
export const timelineStyles = tv({
  slots: {
    legend: 'mb-3 flex flex-wrap items-center gap-x-7 gap-y-2 text-xs text-secondary',
    title: 'font-strong text-primary',
    item: 'flex items-center gap-3',
    hint: 'ml-auto text-muted',
    box: 'relative cursor-pointer select-none',
    chart: 'block size-full',
    head: 'absolute top-1 bottom-9',
    tip: 'absolute -top-13 rounded-md bg-surface-3 px-4 py-1 text-xs whitespace-nowrap',
  },
});
