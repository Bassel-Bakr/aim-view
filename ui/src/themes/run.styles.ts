import { tv } from 'tailwind-variants/lite';

/** The run page: its header row, toolbar, notes and the review's progress. */
export const runStyles = tv({
  slots: {
    head: 'flex flex-wrap items-start justify-between gap-x-7 gap-y-4',
    header: 'min-w-0 flex-1',
    toolbar: 'flex items-center gap-3 rounded-lg border border-border bg-surface-1 p-2',
    note: 'mt-2 text-xs text-muted',
    progress: 'mt-4 flex items-center gap-5 text-xs text-secondary',
    bar: 'w-(--progress-width) accent-accent',
    player: 'mt-6',
  },
});
