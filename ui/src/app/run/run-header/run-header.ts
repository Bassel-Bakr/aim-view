import { DecimalPipe } from '@angular/common';
import { Component, computed, input } from '@angular/core';
import { Recording } from '../../api';
import { formatSize, KIND_LABELS } from '../../format';
import { StampPipe } from '../../stamp-pipe';

/** The open recording's title, kind and data source, and its score, time and size. */
@Component({
  selector: 'app-run-header',
  imports: [DecimalPipe, StampPipe],
  templateUrl: './run-header.html',
  styleUrl: './run-header.scss',
})
export class RunHeader {
  readonly recording = input.required<Recording>();
  protected readonly kindLabels = KIND_LABELS;
  protected readonly size = computed(() => formatSize(this.recording().size));
}
