import { TestBed } from '@angular/core/testing';
import { ClickReport, Report, Source } from '../../api';
import { recording } from '../../fake-api';
import { RunHeader } from './run-header';

/** A report whose kills came from the source. */
function from(source: Source): Report {
  return { mode: 'click', summary: { info: { source } } } as ClickReport;
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

  it('says whether there is a stats file before there is a review', async () => {
    expect((await label(null)).textContent?.trim()).toBe('Stats file');
    expect((await label(null, false)).textContent?.trim()).toBe('No stats file');
  });
});
