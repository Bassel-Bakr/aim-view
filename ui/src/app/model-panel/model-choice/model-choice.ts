import { Component, computed, inject, signal } from '@angular/core';
import { Badge } from '../../controls/badge';
import { Button } from '../../controls/button';
import { Device, errorMessage } from '../../api';
import { DataCell, DataHeader } from '../../data-table/data-cell';
import { DataColumn } from '../../data-table/data-column';
import { DataTable } from '../../data-table/data-table';
import { modelName, Models } from '../../services/models';
import { Review } from '../../services/review';
import { ModelCell, ModelColumn, ModelRow, modelTable } from '../model-table';

/** A measure's row of the model table: its label and about, and each model's cell. */
export interface MeasureLine {
  kind: 'measure';
  row: ModelRow;
}

/** The row of Use buttons under the measures. */
export interface PickLine {
  kind: 'pick';
}

/** A row of the model table, in the app's data table: a measure's, or the Use buttons. */
export type ModelLine = MeasureLine | PickLine;

/** Where the browser runs the detector, as the choice says it. */
const RUNS_ON: Record<Device, string> = {
  cuda: 'GPU',
  cpu: 'CPU',
  wasm: 'CPU',
  webgpu: 'GPU',
  directml: 'GPU',
};

/**
 * The models dialog's body: where the detector runs, the frames it takes at once, and the models side by side (what
 * each does best, its checks and speeds), where another one can be picked. New reviews use the pick; a recording's
 * reviews by other models stay. The panel loads it on its own, so the first screen does without it.
 */
@Component({
  selector: 'app-model-choice',
  imports: [Button, Badge, DataTable, DataCell, DataHeader],
  templateUrl: './model-choice.html',
  styleUrl: './model-choice.scss',
})
export class ModelChoice {
  protected readonly models = inject(Models);
  private readonly review = inject(Review);
  protected readonly runsOn = RUNS_ON;
  protected readonly table = computed(() => {
    const list = this.models.current();
    return list ? modelTable(list) : null;
  });
  /** The measures, then the row of Use buttons. */
  protected readonly lines = computed((): ModelLine[] => {
    const table = this.table();
    if (!table) return [];
    return [...table.rows.map((row): ModelLine => ({ kind: 'measure', row })), { kind: 'pick' }];
  });
  /** The measure's name, then a column a model; their cells are this component's templates. */
  protected readonly columns = computed((): DataColumn<ModelLine>[] => [
    {
      id: 'measure',
      header: '',
      text: (line) => (line.kind === 'measure' ? line.row.label : 'Pick'),
      rowHeader: true,
      wrap: true,
      align: 'start',
    },
    ...(this.table()?.columns ?? []).map((column, index): DataColumn<ModelLine> => ({
      id: column.name,
      header: column.label,
      text: (line) => (line.kind === 'measure' ? (line.row.cells[index]?.text ?? '–') : ''),
      align: 'end',
    })),
  ]);
  private readonly byName = computed(
    () => new Map((this.table()?.columns ?? []).map((column, index) => [column.name, index])),
  );
  protected readonly lineId = (line: ModelLine): string =>
    line.kind === 'measure' ? line.row.label : 'pick';
  protected readonly switching = signal(false);
  protected readonly status = signal('');

  protected columnOf(name: string): ModelColumn | null {
    const index = this.byName().get(name);
    return index === undefined ? null : (this.table()?.columns[index] ?? null);
  }

  protected cellOf(row: ModelRow, name: string): ModelCell | null {
    const index = this.byName().get(name);
    return index === undefined ? null : (row.cells[index] ?? null);
  }

  /** Clears the last switch's word, for the dialog opening again. */
  clearStatus(): void {
    this.status.set('');
  }

  protected useDevice(device: Device): void {
    void this.models.useDevice(device);
  }

  protected useBatch(batch: number): void {
    void this.models.useBatch(batch);
  }

  /** Picks the model new reviews use; the open recording shows its review by that model, when it has one. */
  protected async use(name: string): Promise<void> {
    this.switching.set(true);
    this.status.set(`Loading ${modelName(name)}…`);
    try {
      await this.models.pick(name);
    } catch (error) {
      this.status.set(`Could not switch: ${errorMessage(error)}`);
      return;
    } finally {
      this.switching.set(false);
    }
    this.status.set(`Now using ${modelName(name)}. New reviews use it.`);
    this.review.report.reload();
  }
}
