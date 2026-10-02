import { tv } from 'tailwind-variants/lite';

/** The model panel: its button in the top bar, and the dialog with the models side by side. */
export const modelPanelStyles = tv({
  slots: {
    trigger:
      'rounded-pill border border-border-strong px-4 py-1 text-xs text-secondary hover:bg-surface-2 hover:text-primary',
    down: 'rounded-pill border border-border-strong px-4 py-1 text-xs text-warning',
    dialog:
      'm-auto max-h-(--dialog-max-height) w-(--dialog-width) max-w-full flex-col gap-5 overflow-y-auto rounded-lg border border-border-strong bg-surface-1 p-11 text-primary backdrop:bg-backdrop open:flex',
    head: 'flex items-center justify-between gap-7',
    intro: 'max-w-(--prose-width) text-sm text-secondary',
    scroll: 'flex-none overflow-x-auto',
    table: 'w-full',
    column: 'align-bottom text-sm text-primary',
    rowHead: 'align-top',
    about: 'block font-regular whitespace-normal',
    cell: 'align-top',
    best: 'align-top font-strong text-good-text',
    prose:
      'w-(--model-prose-width) min-w-(--model-prose-width) text-left align-top whitespace-normal text-secondary',
    muted: 'text-muted',
    older: 'text-sm',
    summary: 'cursor-pointer text-secondary',
    olderList: 'mt-3 flex flex-col gap-2',
    olderRow: 'flex items-center gap-6',
    status: 'text-sm text-secondary',
  },
});
