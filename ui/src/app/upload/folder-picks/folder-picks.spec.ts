import { TestBed } from '@angular/core/testing';
import { answer, serverMode } from '../../fake-api';
import { StatsFiles } from '../../platform/stats-files';
import { UploadState } from '../upload-state';
import { FolderPicks } from './folder-picks';

/** Stats files whose folder choice answers with `read`. */
function statsFolder(read: () => Promise<void>): Partial<StatsFiles> {
  return { chooseFolder: read };
}

/** Renders FolderPicks with the stats folder choice given; returns its element. */
async function render(stats: Partial<StatsFiles>): Promise<HTMLElement> {
  TestBed.configureTestingModule({
    imports: [FolderPicks],
    providers: [...serverMode(), { provide: StatsFiles, useValue: stats }],
  });
  const fixture = TestBed.createComponent(FolderPicks);
  await answer({ '/api/vods': [] });
  await fixture.whenStable();
  return fixture.nativeElement as HTMLElement;
}

/** Chooses a folder holding one stats file in the Stats folder input. */
function chooseStatsFolder(el: HTMLElement): void {
  const input = el.querySelector<HTMLInputElement>('input[webkitdirectory]');
  if (!input) throw new Error('no stats folder input');
  const file = new File(['Kill #,Timestamp'], 'Air - Challenge - 2026.10.01-16.23.03 Stats.csv');
  Object.defineProperty(input, 'files', { configurable: true, value: [file] });
  input.dispatchEvent(new Event('change'));
}

describe('FolderPicks', () => {
  it('says beside Upload that the stats folder is read', async () => {
    const el = await render(statsFolder(async () => undefined));
    expect(el.textContent).toContain('Stats folder');
    chooseStatsFolder(el);
    await new Promise((resolve) => setTimeout(resolve));
    expect(TestBed.inject(UploadState).note()).toEqual({
      text: "KovaaK's stats folder is read",
      failed: false,
    });
  });

  it('says why a stats folder could not be read', async () => {
    const el = await render(
      statsFolder(async () => {
        throw new Error('no stats files in it');
      }),
    );
    chooseStatsFolder(el);
    await new Promise((resolve) => setTimeout(resolve));
    expect(TestBed.inject(UploadState).note()).toEqual({
      text: 'Could not read the folder: no stats files in it',
      failed: true,
    });
  });

  it('offers no Stats folder where the server reads the folder itself', async () => {
    const el = await render({ chooseFolder: null });
    expect(el.textContent).not.toContain('Stats folder');
  });
});
