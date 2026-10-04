import { DirMount, FilesMount, HttpMount, KovaakMount, Mounts, PackStore } from './mounts';
import { ChosenFile, DirEntry } from './service-messages';
import { FsResult } from './service-module';

/** The host_fs ops, by the contract's numbers. */
const READ = 0;
const WRITE = 1;
const MKDIRS = 2;
const REMOVE_FILE = 3;
const REMOVE_DIR = 4;
const REMOVE_DIR_ALL = 5;
const RENAME = 6;
const LIST = 7;
const METADATA = 8;

/** A write's place in the file. */
interface WriteAt {
  at: number;
}

/** A create flag, as the handles' calls take it. */
interface CreateOption {
  create?: boolean;
}

/** A remove's flag, as removeEntry takes it. */
interface RemoveOption {
  recursive?: boolean;
}

/** The worker's sync access handle, as far as the mounts use it. */
interface FakeAccess {
  truncate(size: number): void;
  write(data: Uint8Array, options: WriteAt): number;
  flush(): void;
  close(): void;
}

/**
 * A stand-in for an OPFS file handle. Its File is made from text: jsdom's File reads the test's byte arrays as text
 * (they come from another realm), so the spec writes ASCII only.
 */
class FakeFileHandle {
  readonly kind = 'file';
  bytes = new Uint8Array();

  constructor(
    readonly name: string,
    readonly modified = 0,
  ) {}

  async getFile(): Promise<File> {
    const text = new TextDecoder().decode(this.bytes);
    return new File([text], this.name, { lastModified: this.modified });
  }

  async createSyncAccessHandle(): Promise<FakeAccess> {
    return {
      truncate: (size) => (this.bytes = this.bytes.slice(0, size)),
      write: (data, { at }) => {
        const grown = new Uint8Array(Math.max(this.bytes.length, at + data.length));
        grown.set(this.bytes);
        grown.set(data, at);
        this.bytes = grown;
        return data.length;
      },
      flush: () => undefined,
      close: () => undefined,
    };
  }
}

/** A folder's entry: its name and handle. */
type NamedHandle = [name: string, handle: FakeFileHandle | FakeDirHandle];

/** A stand-in for an OPFS folder handle, with the errors the browser throws. */
class FakeDirHandle {
  readonly kind = 'directory';
  readonly children = new Map<string, FakeFileHandle | FakeDirHandle>();

  constructor(readonly name = '') {}

  async getFileHandle(name: string, options: CreateOption = {}): Promise<FakeFileHandle> {
    const found = this.children.get(name);
    if (found instanceof FakeFileHandle) return found;
    if (found) throw new DOMException('a folder', 'TypeMismatchError');
    if (!options.create) throw new DOMException('missing', 'NotFoundError');
    const made = new FakeFileHandle(name);
    this.children.set(name, made);
    return made;
  }

  async getDirectoryHandle(name: string, options: CreateOption = {}): Promise<FakeDirHandle> {
    const found = this.children.get(name);
    if (found instanceof FakeDirHandle) return found;
    if (found) throw new DOMException('a file', 'TypeMismatchError');
    if (!options.create) throw new DOMException('missing', 'NotFoundError');
    const made = new FakeDirHandle(name);
    this.children.set(name, made);
    return made;
  }

  async removeEntry(name: string, options: RemoveOption = {}): Promise<void> {
    const found = this.children.get(name);
    if (!found) throw new DOMException('missing', 'NotFoundError');
    if (found instanceof FakeDirHandle && found.children.size && !options.recursive)
      throw new DOMException('not empty', 'InvalidModificationError');
    this.children.delete(name);
  }

  async *values(): AsyncGenerator<FakeFileHandle | FakeDirHandle> {
    yield* this.children.values();
  }

  async *entries(): AsyncGenerator<NamedHandle> {
    yield* this.children.entries();
  }
}

/**
 * A TextEncoder for ASCII whose byte arrays are this realm's: jsdom's come from another realm, and the mounts'
 * `instanceof Uint8Array` would take them for a Blob.
 */
class AsciiEncoder {
  encode(text: string): Uint8Array {
    return Uint8Array.from(text, (char) => char.charCodeAt(0));
  }
}

beforeEach(() => vi.stubGlobal('TextEncoder', AsciiEncoder));
afterEach(() => vi.unstubAllGlobals());

function handle(fake: FakeDirHandle): Promise<FileSystemDirectoryHandle> {
  return Promise.resolve(fake as unknown as FileSystemDirectoryHandle);
}

const encode = (text: string) => new TextEncoder().encode(text);
const decode = (result: FsResult) => new TextDecoder().decode(result.bytes);
const runNow = <T>(step: () => Promise<T>) => step();

function chosen(path: string, text: string, modified: number): ChosenFile {
  return { path, file: new File([text], path.split('/').pop() ?? '', { lastModified: modified }) };
}

/** Mounts with a writable folder at /data. */
function dataMounts(): Mounts {
  const mounts = new Mounts();
  mounts.set('data', new DirMount(handle(new FakeDirHandle()), true));
  return mounts;
}

describe('Mounts over a folder handle', () => {
  let mounts: Mounts;
  beforeEach(() => (mounts = dataMounts()));

  it('makes folders, writes, reads, stats and lists', async () => {
    expect((await mounts.host(MKDIRS, '/data/a/b', encode(''))).code).toBe(0);
    expect((await mounts.host(WRITE, '/data/a/b/c.txt', encode('hello'))).code).toBe(0);
    expect(decode(await mounts.host(READ, '/data/a/b/c.txt', encode('')))).toBe('hello');
    expect(JSON.parse(decode(await mounts.host(METADATA, '/data/a/b/c.txt', encode(''))))).toEqual({
      dir: false,
      len: 5,
      modified: 0,
    });
    expect(JSON.parse(decode(await mounts.host(LIST, '/data/a', encode(''))))).toEqual([
      ['b', true, 0, 0],
    ]);
    expect(await mounts.list('/data/a/b', 0)).toEqual([['c.txt', false, 5, 0]]);
    expect(await (await mounts.file('/data/a/b/c.txt')).text()).toBe('hello');
    expect(await mounts.exists('/data/a/b')).toBe(true);
    expect(await mounts.exists('/data/a/none')).toBe(false);
  });

  it('answers the root and refuses what it cannot do, with the codes', async () => {
    expect(JSON.parse(decode(await mounts.host(METADATA, '/', encode(''))))).toEqual({
      dir: true,
      len: 0,
      modified: 0,
    });
    const missing = await mounts.host(READ, '/data/none.txt', encode(''));
    expect([missing.code, decode(missing)]).toEqual([1, '/data/none.txt: not found']);
    expect((await mounts.host(READ, 'data/x', encode(''))).code).toBe(3);
    expect((await mounts.host(READ, '/data/../x', encode(''))).code).toBe(3);
    expect((await mounts.host(READ, '/nowhere/x', encode(''))).code).toBe(1);
    const unknown = await mounts.host(9, '/data/x', encode(''));
    expect([unknown.code, decode(unknown)]).toEqual([3, 'no file system call 9']);
    await expect(mounts.file('/data/none.txt')).rejects.toMatchObject({ code: 1 });
  });

  it('removes files and folders, a full folder only with all', async () => {
    await mounts.write('/data/a/b/c.txt', encode('hello'));
    expect((await mounts.host(REMOVE_DIR, '/data/a', encode(''))).code).toBe(2);
    expect((await mounts.host(REMOVE_FILE, '/data/a/b/c.txt', encode(''))).code).toBe(0);
    expect((await mounts.host(REMOVE_FILE, '/data/a/b/c.txt', encode(''))).code).toBe(1);
    await mounts.write('/data/a/b/d.txt', encode('x'));
    expect((await mounts.host(REMOVE_DIR_ALL, '/data/a', encode(''))).code).toBe(0);
    expect(await mounts.exists('/data/a/b/d.txt')).toBe(false);
    await mounts.write('/data/a/e.txt', encode('again'));
    expect(decode(await mounts.host(READ, '/data/a/e.txt', encode('')))).toBe('again');
  });

  it('moves files and folders, but not to another mount', async () => {
    await mounts.write('/data/a/b/c.txt', encode('hello'));
    expect((await mounts.host(RENAME, '/data/a/b/c.txt', encode('/data/a/d.txt'))).code).toBe(0);
    expect(await mounts.exists('/data/a/b/c.txt')).toBe(false);
    expect(decode(await mounts.host(READ, '/data/a/d.txt', encode('')))).toBe('hello');
    expect((await mounts.host(RENAME, '/data/a', encode('/data/moved'))).code).toBe(0);
    expect(decode(await mounts.host(READ, '/data/moved/d.txt', encode('')))).toBe('hello');
    expect(await mounts.exists('/data/a')).toBe(false);
    mounts.set('vods', new DirMount(handle(new FakeDirHandle()), false));
    expect((await mounts.host(RENAME, '/data/moved', encode('/vods/x'))).code).toBe(3);
    expect(decode(await mounts.host(WRITE, '/vods/x.txt', encode('no')))).toBe('read-only');
  });
});

describe('Mounts copying into a folder', () => {
  it('copies only what is new or changed, keeping its index', async () => {
    const mounts = dataMounts();
    const files = [chosen('one.csv', 'first', 1000), chosen('sub/two.csv', 'second', 2000)];
    const progress: number[][] = [];
    const record = (done: number, total: number) => progress.push([done, total]);
    expect(await mounts.copyIn('/data/stats', files, runNow, record)).toEqual({ copied: 2 });
    expect(progress).toEqual([
      [0, 2],
      [2, 2],
    ]);
    expect(await (await mounts.file('/data/stats/sub/two.csv')).text()).toBe('second');
    expect(JSON.parse(await (await mounts.file('/data/stats/copied.json')).text())).toEqual({
      'one.csv': [5, 1000],
      'sub/two.csv': [6, 2000],
    });
    expect(await mounts.copyIn('/data/stats', files, runNow, record)).toEqual({ copied: 0 });
    const changed = [chosen('one.csv', 'first', 3000), files[1]];
    expect(await mounts.copyIn('/data/stats', changed, runNow, record)).toEqual({ copied: 1 });
  });
});

describe('Mounts over chosen files', () => {
  const mounts = new Mounts();
  mounts.set('vods', new FilesMount([chosen('a/b.mp4', 'video', 4000), chosen('top.mp4', 'v', 0)]));

  it('lists, stats and reads them, read-only', async () => {
    expect(await mounts.list('/vods', 0)).toEqual([
      ['a', true, 0, 0],
      ['top.mp4', false, 1, 0],
    ]);
    expect(await mounts.list('/vods', 1)).toEqual([['a', true, 0, 0]]);
    expect(await mounts.list('/vods/a', 0)).toEqual([['b.mp4', false, 5, 4]]);
    expect(await (await mounts.file('/vods/a/b.mp4')).text()).toBe('video');
    expect(JSON.parse(decode(await mounts.host(METADATA, '/vods/a', encode(''))))).toEqual({
      dir: true,
      len: 0,
      modified: 0,
    });
    expect((await mounts.host(READ, '/vods/a', encode(''))).code).toBe(3);
    expect((await mounts.host(READ, '/vods/c.mp4', encode(''))).code).toBe(1);
    expect((await mounts.host(MKDIRS, '/vods/a', encode(''))).code).toBe(0);
    expect((await mounts.host(MKDIRS, '/vods/z', encode(''))).code).toBe(3);
    expect((await mounts.host(WRITE, '/vods/a/c.mp4', encode('x'))).code).toBe(3);
    expect((await mounts.host(REMOVE_FILE, '/vods/top.mp4', encode(''))).code).toBe(3);
  });
});

describe('Mounts over KovaaK files', () => {
  let packRoot: FakeDirHandle;
  let older: FakeDirHandle;
  let mounts: Mounts;
  beforeEach(async () => {
    packRoot = new FakeDirHandle();
    older = new FakeDirHandle();
    const olderMount = new DirMount(handle(older), true);
    await olderMount.mkdirs(['stats']);
    await olderMount.write(['stats', 'old.csv'], encode('older'));
    mounts = new Mounts();
    mounts.set('kovaak', new KovaakMount(new PackStore(handle(packRoot)), olderMount));
  });

  it('copies into packs, reads them back, and copies again only what changed', async () => {
    const files = [chosen('stats/a.csv', 'alpha', 1000), chosen('stats/b.csv', 'beta', 2000)];
    const progress: number[][] = [];
    const record = (done: number, total: number) => progress.push([done, total]);
    expect(await mounts.copyIn('/kovaak', files, runNow, record)).toEqual({ copied: 2 });
    expect(progress).toEqual([
      [0, 2],
      [2, 2],
    ]);
    expect(await (await mounts.file('/kovaak/stats/b.csv')).text()).toBe('beta');
    expect((await mounts.file('/kovaak/stats/b.csv')).lastModified).toBe(2000);
    const entries: DirEntry[] = await mounts.list('/kovaak/stats', 0);
    expect(entries).toEqual([
      ['a.csv', false, 5, 1],
      ['b.csv', false, 4, 2],
      ['old.csv', false, null, null],
    ]);
    expect(
      JSON.parse(decode(await mounts.host(METADATA, '/kovaak/stats/a.csv', encode('')))),
    ).toEqual({ dir: false, len: 5, modified: 1 });
    expect(await mounts.copyIn('/kovaak', files, runNow, record)).toEqual({ copied: 0 });
    const changed = [files[0], chosen('stats/b.csv', 'beta2', 2000)];
    expect(await mounts.copyIn('/kovaak', changed, runNow, record)).toEqual({ copied: 1 });
    expect(await (await mounts.file('/kovaak/stats/b.csv')).text()).toBe('beta2');
  });

  it('reads the packs again from their index, and the older copies under them', async () => {
    await mounts.copyIn('/kovaak', [chosen('stats/a.csv', 'alpha', 1000)], runNow, () => undefined);
    const again = new Mounts();
    again.set(
      'kovaak',
      new KovaakMount(new PackStore(handle(packRoot)), new DirMount(handle(older), true)),
    );
    expect(await (await again.file('/kovaak/stats/a.csv')).text()).toBe('alpha');
    expect(await (await again.file('/kovaak/stats/old.csv')).text()).toBe('older');
    expect((await again.host(READ, '/kovaak/stats/none.csv', encode(''))).code).toBe(1);
    expect((await again.host(LIST, '/kovaak/none', encode(''))).code).toBe(1);
    expect((await again.host(WRITE, '/kovaak/stats/a.csv', encode('x'))).code).toBe(3);
  });
});

describe('Mounts over KovaaK files chosen this visit', () => {
  it('shows them over the kept copies', async () => {
    const older = new FakeDirHandle();
    const olderMount = new DirMount(handle(older), true);
    await olderMount.mkdirs(['stats']);
    await olderMount.write(['stats', 'old.csv'], encode('older'));
    const kovaak = new KovaakMount(new PackStore(handle(new FakeDirHandle())), olderMount);
    kovaak.show([chosen('stats/old.csv', 'chosen', 7000)]);
    const mounts = new Mounts();
    mounts.set('kovaak', kovaak);
    expect(await (await mounts.file('/kovaak/stats/old.csv')).text()).toBe('chosen');
    expect(await mounts.list('/kovaak/stats', 0)).toEqual([['old.csv', false, 6, 7]]);
  });
});

describe('Mounts over the models folder', () => {
  /** A stand-in for fetch's answer, its body made with jsdom's own Blob. */
  function answer(status: number, body: string, type = 'application/json'): Response {
    const headers = new Headers({ 'content-type': type, 'content-length': String(body.length) });
    const fake = {
      status,
      ok: status < 400,
      statusText: 'status',
      headers,
      blob: async () => new Blob([body]),
    };
    return fake as unknown as Response;
  }

  const mounts = new Mounts();
  mounts.set('models', new HttpMount('http://test/models/'));
  beforeEach(() =>
    vi.stubGlobal('fetch', async (url: string) => {
      if (url.endsWith('/models.json')) return answer(200, '{"models":{"m1":{}}}');
      if (url.endsWith('/detector_m1.json')) return answer(200, '{"size":1}');
      if (url.endsWith('/page')) return answer(200, '<html>', 'text/html');
      if (url.endsWith('/broken')) return answer(500, '');
      return answer(404, '');
    }),
  );

  it('lists models.json and each model it names, and stats and reads them', async () => {
    expect(await mounts.list('/models', 0)).toEqual([
      ['models.json', false, null, null],
      ['detector_m1.json', false, null, null],
      ['detector_m1_u8in.onnx', false, null, null],
    ]);
    expect(await (await mounts.file('/models/detector_m1.json')).text()).toBe('{"size":1}');
    expect(
      JSON.parse(decode(await mounts.host(METADATA, '/models/detector_m1.json', encode('')))),
    ).toEqual({ dir: false, len: 10, modified: 0 });
    expect((await mounts.host(READ, '/models/page', encode(''))).code).toBe(1);
    expect((await mounts.host(READ, '/models/none', encode(''))).code).toBe(1);
    expect((await mounts.host(READ, '/models/broken', encode(''))).code).toBe(3);
    expect((await mounts.host(READ, '/models/a/b', encode(''))).code).toBe(1);
    expect((await mounts.host(MKDIRS, '/models', encode(''))).code).toBe(0);
  });
});
