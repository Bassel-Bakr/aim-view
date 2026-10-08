import { TestBed } from '@angular/core/testing';
import { ClickReport, Report, Source, TrackReport } from '../../api';
import { recording } from '../../fake-api';
import { RunHeader } from './run-header';

/** A report whose kills came from the source. */
function from(source: Source): Report {
  return { mode: 'click', summary: { info: { source } } } as ClickReport;
}

/** A tracking run's report whose score and deaths came from the source. */
function trackedFrom(source: Source): Report {
  return { mode: 'track', summary: { info: { source } } } as TrackReport;
}

async function label(report: Report | null, stats = true): Promise<HTMLElement> {
  const fixture = TestBed.createComponent(RunHeader);
  fixture.componentRef.setInput('recording', recording({ stats }));
  fixture.componentRef.setInput('report', report);
  await fixture.whenStable();
  return (fixture.nativeElement as HTMLElement).querySelector('.source') as HTMLElement;
}

describe('RunHeader', () => {
  it('says where the review’s kills came from, the stats file as good news', async () => {
    const stats = await label(from('stats'));
    expect(stats.textContent?.trim()).toBe('Kills from the stats file');
    expect(stats.classList).toContain('badge');
    expect(stats.dataset['tone']).toBe('good');
    expect((await label(from('hud'))).textContent?.trim()).toBe("Kills read from KovaaK's HUD");
    expect((await label(from('aimlab'))).textContent?.trim()).toBe("Kills read from Aim Lab's HUD");
    const video = await label(from('video'));
    expect(video.textContent?.trim()).toBe(
      'Kills from the video alone (no score, shots or accuracy)',
    );
    expect(video.dataset['tone']).toBe('neutral');
  });

  it('says where a tracking run’s score came from, not its kills', async () => {
    expect((await label(trackedFrom('stats'))).textContent?.trim()).toBe(
      'Score from the stats file',
    );
    const video = await label(trackedFrom('video'));
    expect(video.textContent?.trim()).toBe('From the video alone (no score or accuracy)');
    expect(video.dataset['tooltip']).toBe(
      'The time on the target measured in the video alone: no score or accuracy',
    );
  });

  it('says whether there is a stats file before there is a review', async () => {
    expect((await label(null)).textContent?.trim()).toBe('Stats file');
    expect((await label(null, false)).textContent?.trim()).toBe('No stats file');
  });
});
