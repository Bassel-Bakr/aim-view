import { Component, input } from '@angular/core';
import { HeadlineTile } from '../report/click-stats';

/** The run in a few big numbers, above the video. A flagged tile is the review's "work on this". */
@Component({
  selector: 'app-headline',
  templateUrl: './headline.html',
  styleUrl: './headline.scss',
})
export class Headline {
  readonly tiles = input.required<HeadlineTile[]>();
}
