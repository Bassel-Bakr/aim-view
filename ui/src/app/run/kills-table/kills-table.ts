import { Component, computed, input, output } from '@angular/core';
import { Flick } from '../../api';
import { DataColumn, SortValue, TableGrouping } from '../../data-table/data-column';
import { DataTable } from '../../data-table/data-table';
import { median } from '../median';
import { FlickRow, GROUPINGS } from './kill-rows';

/**
 * A clicking run's kills in the app's data table (data-table.ts), one row each (kill-rows.ts says what each cell
 * holds): a column header sorts by it, the Group by menu groups the kills (how the flick landed, its direction, its
 * distance, the first shot, the spawn), each group's row giving its count and median TTK, and the Columns menu hides
 * columns (kept in this browser). A row's click plays its kill (playKill); the kill in focus is marked and kept in view.
 */

/** Seconds as milliseconds; nothing when not measured. */
const ms = (seconds: number | null | undefined): SortValue =>
  seconds == null ? undefined : 1000 * seconds;
const measured = (value: number | null | undefined): SortValue => value ?? undefined;

export const KILL_COLUMNS: readonly DataColumn<FlickRow>[] = [
  {
    id: 'kill',
    header: 'Kill',
    title: 'The kill, in order',
    text: (row) => String(row.killNumber),
    sortBy: (row) => row.killNumber,
    rowHeader: true,
  },
  {
    id: 'distance',
    header: 'Dist',
    title: 'Distance to the target, and the way it was',
    text: (row) => row.distance,
    sortBy: (row) => row.flick.D0,
    align: 'end',
  },
  {
    id: 'ttk',
    header: 'TTK ms',
    title: 'TTK: time to kill, in milliseconds',
    text: (row) => row.ttk,
    sortBy: (row) => ms(row.flick.total),
  },
  {
    id: 'landed',
    header: 'Landed',
    title:
      'Flick landed: on target, an underflick (the degrees still to go) or an overflick (the degrees past the far edge)',
    text: (row) => row.landed,
    sortBy: (row) => measured(row.flick.end_left),
  },
  {
    id: 'confirmation',
    header: 'Conf ms',
    title: 'Confirmation: still on the target before the click, in milliseconds',
    text: (row) => row.confirmation,
    sortBy: (row) => ms(row.flick.still),
  },
  {
    id: 'speed',
    header: 'Speed °/s',
    title: 'Flick speed: the highest speed in the flick, in degrees a second',
    text: (row) => row.flickSpeed,
    sortBy: (row) => measured(row.flick.peak),
  },
  {
    id: 'onMove',
    header: 'On move °/s',
    title: "Click on the move: the crosshair's speed at the click, in degrees a second",
    text: (row) => row.onTheMove,
    sortBy: (row) => measured(row.flick.click_speed),
  },
  {
    id: 'shots',
    header: 'Shots',
    title: 'Shots at this target',
    text: (row) => row.shots,
    sortBy: (row) => measured(row.flick.shots),
    tone: (row) => (row.missed ? 'attention' : null),
  },
  {
    id: 'pathing',
    header: 'Pathing',
    title: 'Pathing: what picking this target cost against the fastest pick',
    text: (row) => row.pathing,
    sortBy: (row) => measured(row.pickCost),
    align: 'end',
  },
  {
    id: 'steps',
    header: 'React · Flick · Micro ms',
    title:
      "Reaction, flick (the main movement) and micro (from the flick's end onto the target, then settling; each row splits it on hover)",
    text: (row) => row.steps,
    sortable: false,
    tip: (row) => row.microSplit,
    align: 'end',
  },
  {
    id: 'offCenter',
    header: 'Off ctr',
    title: "Click off center: how far the crosshair was from the target's center at the click",
    text: (row) => row.offCenter,
    sortBy: (row) => measured(row.flick.click_off),
  },
  {
    id: 'micros',
    header: 'Micros',
    title: 'Micros: separate movements after the flick',
    text: (row) => row.micros,
    sortBy: (row) => measured(row.flick.corrections),
  },
  {
    id: 'spawn',
    header: 'Spawn',
    title: 'Spawn: the target appeared after the flick began',
    text: (row) => row.spawn,
    sortBy: (row) => (row.flick.spawned ? 1 : 0),
  },
];

/** A group of kills' median TTK: "median TTK 600 ms". */
function medianTtk(rows: readonly FlickRow[]): string {
  const ttk = median(
    rows.flatMap((row) => (row.flick.total == null ? [] : [1000 * row.flick.total])),
  );
  return ttk === null ? '' : `median TTK ${Math.round(ttk)} ms`;
}

export const KILL_GROUPINGS: readonly TableGrouping<FlickRow>[] = GROUPINGS.flatMap((choice) => {
  const key = choice.key;
  return key === null
    ? []
    : [
        {
          label: choice.label,
          key: (row: FlickRow) => row.groups[key],
          noun: 'kill',
          summary: medianTtk,
        },
      ];
});

@Component({
  selector: 'app-kills-table',
  imports: [DataTable],
  templateUrl: './kills-table.html',
  styleUrl: './kills-table.scss',
})
export class KillsTable {
  readonly rows = input.required<FlickRow[]>();
  readonly selected = input<Flick | null>(null);
  readonly playKill = output<Flick>();
  protected readonly columns = KILL_COLUMNS;
  protected readonly groupings = KILL_GROUPINGS;
  protected readonly selectedId = computed(() => {
    const flick = this.selected();
    return flick ? String(flick.kill_number) : null;
  });
  protected readonly rowId = (row: FlickRow): string => String(row.killNumber);
  protected readonly pickLabel = (row: FlickRow): string => `Play kill ${row.killNumber}`;
}
