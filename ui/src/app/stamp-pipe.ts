import { Pipe, PipeTransform } from '@angular/core';
import { formatStamp } from './format';

/** A recording's time stamp as "Aug 11, 13:55" (formatStamp). Pure, so a row's text is computed once. */
@Pipe({ name: 'stamp' })
export class StampPipe implements PipeTransform {
  transform(stamp: string): string {
    return formatStamp(stamp);
  }
}
