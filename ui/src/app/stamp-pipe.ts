/**
 * The `stamp` pipe (`StampPipe`): a recording's time stamp in the page's words. In: a stamp as
 * KovOBS names files. Out: the templates that show one (the recordings list).
 */

import { Pipe, PipeTransform } from '@angular/core';
import { formatStamp } from './format';

/** A recording's time stamp as "Aug 11, 13:55" (formatStamp). Pure, so a row's text is computed once. */
@Pipe({ name: 'stamp' })
export class StampPipe implements PipeTransform {
  /** The stamp ("2026.08.11-13.55.02") as "Aug 11, 13:55"; the stamp itself when it is not one. */
  transform(stamp: string): string {
    return formatStamp(stamp);
  }
}
