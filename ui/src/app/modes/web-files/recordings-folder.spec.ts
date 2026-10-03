import { TestBed } from '@angular/core/testing';
import { BrowserStore } from './browser-store';
import { RecordingsFolder } from './recordings-folder';

const gone = () =>
  new DOMException('A requested file or directory could not be found', 'NotFoundError');

/** A file handle; gone: deleted since its folder was listed. */
const fileHandle = (name: string, isGone = false) => ({
  kind: 'file',
  name,
  getFile: async () => {
    if (isGone) throw gone();
    return new File(['v'], name);
  },
});

/** A remembered folder handle the browser lets the page read; entries() throws when the folder is gone. */
function folder(name: string, entries: ReturnType<typeof fileHandle>[] | 'gone') {
  return {
    kind: 'directory',
    name,
    queryPermission: async () => 'granted',
    requestPermission: async () => 'granted',
    async *entries() {
      if (entries === 'gone') throw gone();
      for (const e of entries) yield [e.name, e];
    },
  } as unknown as FileSystemDirectoryHandle;
}

/** The recordings folder on a page whose store remembers this handle. */
function remembering(handle: FileSystemDirectoryHandle): RecordingsFolder {
  TestBed.configureTestingModule({
    providers: [{ provide: BrowserStore, useValue: { get: async () => handle } }],
  });
  return TestBed.inject(RecordingsFolder);
}

describe('RecordingsFolder', () => {
  it('says a remembered folder cannot be found, without failing, and keeps it remembered', async () => {
    const f = remembering(folder('KovOBS', 'gone'));
    await expect(f.restore()).resolves.toBeNull();
    expect(f.state()).toMatchObject({ name: null, gone: 'KovOBS', busy: false, refused: null });
  });

  it('leaves out a video gone since the folder was listed', async () => {
    const f = remembering(folder('KovOBS', [fileHandle('a.mp4'), fileHandle('b.mp4', true)]));
    const found = await f.restore();
    expect(found?.map((e) => e.path)).toEqual(['a.mp4']);
    expect(f.state()).toMatchObject({ name: 'KovOBS', gone: null });
  });
});
