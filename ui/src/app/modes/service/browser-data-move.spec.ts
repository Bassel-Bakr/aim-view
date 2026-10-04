import { moveBrowserData } from './browser-data-move';
import { ServiceAnswer, ServiceMethod } from './service-messages';

/** A call the move sent to the service: its method, path and body as text. */
interface SentCall {
  method: ServiceMethod;
  path: string;
  body: string;
}

/** A stand-in for an IndexedDB request: it succeeds with its result once the caller has set its handlers. */
class FakeRequest<T> {
  readonly transaction = null;
  onsuccess: (() => void) | null = null;
  onerror: (() => void) | null = null;
  onupgradeneeded: (() => void) | null = null;

  constructor(readonly result: T) {
    setTimeout(() => this.onsuccess?.(), 0);
  }
}

/** A stand-in for the old store's transaction: gets and puts on its values. */
class FakeTransaction {
  oncomplete: (() => void) | null = null;
  onerror: (() => void) | null = null;

  constructor(private readonly values: Map<string, unknown>) {}

  objectStore(): FakeTransaction {
    return this;
  }

  get(key: string): FakeRequest<unknown> {
    return new FakeRequest(this.values.get(key));
  }

  put(value: unknown, key: string): void {
    this.values.set(key, value);
    setTimeout(() => this.oncomplete?.(), 0);
  }
}

/** The old store ("aimview", its "kv" object store) with its values. */
class FakeDatabase {
  readonly objectStoreNames = { contains: (name: string) => name === 'kv' };

  constructor(readonly values: Map<string, unknown>) {}

  transaction(): FakeTransaction {
    return new FakeTransaction(this.values);
  }

  close(): void {
    // nothing to close
  }
}

/** A folder entry the old VODs folder lists: its name and its handle. */
type FakeEntry = [name: string, handle: FakeFolder | FakeVideo];

/** A file handle of the old VODs folder. */
class FakeVideo {
  readonly kind = 'file';

  constructor(readonly file: File) {}

  async getFile(): Promise<File> {
    return this.file;
  }
}

/** The old VODs folder's handle, readable as asked. */
class FakeFolder {
  readonly kind = 'directory';

  constructor(
    private readonly children: FakeEntry[],
    private readonly permission = 'granted',
  ) {}

  async queryPermission(): Promise<string> {
    return this.permission;
  }

  async *entries(): AsyncGenerator<FakeEntry> {
    yield* this.children;
  }
}

const VIDEO = new File(['v'], 'run.mp4', { lastModified: 5 });
const PRINT = 'run.mp4|1|5';
const OTHER_PRINT = 'gone.mp4|9|9';

/** The old store as a user of the old browser mode left it. */
function oldStore(folder: FakeFolder): Map<string, unknown> {
  return new Map<string, unknown>([
    ['review-index', { [PRINT]: { m1: 1 }, [OTHER_PRINT]: { m1: 1 } }],
    [`review:${PRINT}|m1`, { tracks: { frames: 1 }, readings: { shifts: 2 }, model: 'm1' }],
    ['run-marks', { [PRINT]: { start: 1, end: 2 } }],
    ['faint-cutoffs', { [PRINT]: { offset: 3 } }],
    ['faint-skipped', [PRINT]],
    ['exclude-areas', { [PRINT]: [] }],
    ['mouse-logs', { 'log-print': 'mouse.csv' }],
    ['mouse-log|log-print', Uint8Array.from([108, 111, 103]).buffer],
    ['not-aim', ['sub/a.mp4']],
    ['label-skipped', ['b.mp4']],
    ['area-examples', [{ rec: 'run.mp4', boxes: 1 }]],
    ['area-kinds', [{ id: 'hud', name: 'HUD', about: 'the HUD' }]],
    [
      'stats-pairs',
      {
        'folder:run.mp4': { stats: { name: 's.csv', text: 'csv' }, how: 'picked' },
        'folder:other.mp4': { stats: { name: 't.csv', text: 'csv' }, how: 'guessed' },
        'upload:x.mp4': { stats: { name: 'u.csv', text: 'csv' }, how: 'picked' },
      },
    ],
    ['recordings-folder', folder],
  ]);
}

/** The service's answers: the shipped examples, and an area type's id turned down once. */
function service(sent: SentCall[]) {
  return async (method: ServiceMethod, path: string, bytes: Uint8Array): Promise<ServiceAnswer> => {
    const body = new TextDecoder().decode(bytes);
    sent.push({ method, path, body });
    const answer = (status: number, text = '') => ({
      status,
      type: 'application/json',
      body: Uint8Array.from(text, (char) => char.charCodeAt(0)),
    });
    if (path === '/api/area_examples' && method === 'GET')
      return answer(200, '{"rec":"run.mp4","boxes":0}\n{"rec":"shipped.mp4","boxes":0}\n');
    if (path === '/api/area_kinds' && body.includes('"hud"')) return answer(409, 'taken');
    return answer(200);
  };
}

describe('moveBrowserData', () => {
  beforeEach(() => vi.spyOn(console, 'warn').mockImplementation(() => undefined));
  afterEach(() => vi.unstubAllGlobals());

  function useStore(values: Map<string, unknown>): void {
    vi.stubGlobal('indexedDB', { open: () => new FakeRequest(new FakeDatabase(values)) });
  }

  it('moves every kept thing into the service once, through its routes', async () => {
    const sub = new FakeFolder([['clip.mkv', new FakeVideo(new File(['c'], 'clip.mkv'))]]);
    const folder = new FakeFolder([
      ['run.mp4', new FakeVideo(VIDEO)],
      ['notes.txt', new FakeVideo(new File(['n'], 'notes.txt'))],
      ['sub', sub],
    ]);
    const values = oldStore(folder);
    useStore(values);
    const sent: SentCall[] = [];
    await moveBrowserData(service(sent));
    expect(sent.map((call) => `${call.method} ${call.path} ${call.body}`)).toEqual([
      'POST /api/folder?path=%2Fvods ',
      'POST /api/run?id=run.mp4 {"start":1,"end":2}',
      'POST /api/reviewed?id=run.mp4 {"model":"m1","tracks":{"frames":1},"readings":{"shifts":2},"hud":null,"found":null}',
      'POST /api/faint?id=run.mp4 {"offset":3}',
      'POST /api/faint_skip?id=run.mp4 ',
      'POST /api/exclude?id=run.mp4 []',
      'POST /api/upload?name=s.csv&id=run.mp4 csv',
      'POST /api/not_aim?id=sub%2Fa.mp4&on=1 ',
      'POST /api/label_skip?id=b.mp4 ',
      'POST /api/area_kinds {"id":"hud","name":"HUD","about":"the HUD"}',
      'POST /api/area_kinds {"id":null,"name":"HUD","about":"the HUD"}',
      'GET /api/area_examples ',
      'POST /api/area_examples {"rec":"shipped.mp4","boxes":0}\n{"rec":"run.mp4","boxes":1}\n',
      'POST /api/mouse_log?name=mouse.csv log',
    ]);
    expect(values.has('moved-to-service')).toBe(true);
    sent.length = 0;
    await moveBrowserData(service(sent));
    expect(sent).toEqual([]);
  });

  it('waits for a visit that can read the folder while recordings have saved data', async () => {
    const values = oldStore(new FakeFolder([['run.mp4', new FakeVideo(VIDEO)]], 'prompt'));
    useStore(values);
    const sent: SentCall[] = [];
    await moveBrowserData(service(sent));
    expect(sent).toEqual([]);
    expect(values.has('moved-to-service')).toBe(false);
  });

  it('marks an empty store moved, and does nothing without one', async () => {
    const values = new Map<string, unknown>();
    useStore(values);
    const sent: SentCall[] = [];
    await moveBrowserData(service(sent));
    expect(values.has('moved-to-service')).toBe(true);
    vi.stubGlobal('indexedDB', undefined);
    await moveBrowserData(service(sent));
    expect(sent).toEqual([]);
  });
});
