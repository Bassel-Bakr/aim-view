import { tv } from 'tailwind-variants/lite';

/** The stats file panel: the file in use, the files to pair with, and what the last change did. */
export const statsFileStyles = tv({
  slots: {
    panel: 'mt-6 flex flex-col gap-5 rounded-lg border border-border bg-surface-1 px-9 py-7',
    head: 'flex items-center justify-between gap-7',
    title: 'm-0',
    intro: 'max-w-(--prose-width) text-sm text-secondary',
    setup: 'flex flex-col gap-3 rounded-md border border-border-strong px-6 py-5',
    current: 'font-strong break-all',
    how: 'font-regular text-muted',
    facts: 'flex flex-wrap gap-x-11 gap-y-3 text-sm',
    factLabel: 'text-xs text-muted',
    actions: 'flex flex-wrap gap-3',
    searchLabel: 'flex max-w-(--prose-width) flex-col gap-2 text-xs text-muted',
    search:
      'w-full rounded-md border border-border-strong bg-surface-0 px-6 py-4 text-sm text-primary placeholder:text-muted',
    list: 'flex max-h-(--candidates-height) flex-col overflow-y-auto rounded-md border border-border',
    row: 'flex min-h-(--control-height) items-center gap-6 border-b border-grid px-6 py-1 text-sm tabular-nums last:border-b-0',
    when: 'w-(--stamp-width) flex-none text-primary',
    off: 'flex-1 text-muted',
    scenario: 'min-w-0 flex-1 truncate text-secondary',
    hint: 'text-xs text-muted',
    failed: 'text-sm text-warning',
  },
});
