import { tv } from 'tailwind-variants/lite';

/** The flick list beside the video: its header, the scrolling table and its rows. */
export const flickListStyles = tv({
  slots: {
    head: 'flex flex-wrap items-center justify-between gap-x-4 gap-y-1 pb-3',
    title: 'font-strong text-primary',
    hint: 'text-xs text-muted',
    scroll: 'min-h-0 flex-1 overflow-y-auto rounded-lg border border-border',
    table: 'w-full',
    th: 'sticky top-0 bg-surface-1 px-3',
    row: 'relative cursor-pointer hover:bg-surface-2 data-selected:bg-accent-soft',
    td: 'px-3',
    // The button's hit area covers its row: a click anywhere in the row plays the flick
    play: 'min-w-(--control-height-sm) rounded-sm px-2 text-left font-strong text-primary after:absolute after:inset-0',
    missed: 'px-3 text-warning',
  },
});
