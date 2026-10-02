import { tv } from 'tailwind-variants/lite';

/** A clicking run's report: its cards, the time budget, the checks and the tables. */
export const clickReportStyles = tv({
  slots: {
    head: 'flex flex-wrap items-baseline gap-x-5 gap-y-2',
    headTitle: 'font-strong text-primary',
    headHint: 'text-xs text-muted',
    cards: 'mt-4 grid grid-cols-(--cards-columns) gap-4',
    budget: 'mt-2 flex flex-col gap-2',
    bar: 'flex h-(--budget-height) overflow-hidden rounded-md',
    thinBar: 'flex h-(--budget-thin-height) overflow-hidden rounded-sm',
    segment:
      'flex min-w-0 items-center overflow-hidden px-2 text-xs whitespace-nowrap text-primary',
    reference: 'flex flex-col gap-1 text-xs text-muted',
    legend: 'mt-2 flex flex-wrap gap-x-7 gap-y-2 text-xs text-secondary',
    legendItem: 'flex items-center gap-3',
    issues: 'grid grid-cols-(--issues-columns) gap-4',
    issue: 'flex flex-col gap-2 rounded-lg border border-border bg-surface-1 px-6 py-5',
    issueTop: 'flex items-start justify-between gap-4 font-strong text-primary',
    attention: 'text-xs whitespace-nowrap text-(--issue-attention)',
    fine: 'text-xs whitespace-nowrap text-(--issue-fine)',
    issueValue: 'text-secondary',
    issueWhy: 'text-xs text-muted',
    tables: 'grid grid-cols-(--tables-columns) gap-x-13',
  },
});
