import { tv } from 'tailwind-variants/lite';

/** The recordings list: its filter, chips and rows. */
export const recordingsStyles = tv({
  slots: {
    search:
      'w-full rounded-md border border-border-strong bg-surface-0 px-6 py-4 text-primary placeholder:text-muted',
    chips: 'flex flex-wrap gap-3',
    count: 'ml-1 text-muted',
    list: '-mx-2 min-h-0 flex-1 overflow-y-auto px-2',
    row: 'cursor-pointer rounded-md border-l-(length:--line-width-marker) border-transparent px-5 py-4 hover:bg-surface-2 aria-selected:border-accent aria-selected:bg-accent-soft',
    name: 'truncate text-sm font-strong',
    meta: 'mt-1 flex flex-wrap items-center gap-x-4 gap-y-2 text-xs text-muted tabular-nums',
    note: 'text-sm text-muted',
  },
});
