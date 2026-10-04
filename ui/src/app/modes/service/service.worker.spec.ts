import {
  ChosenFile,
  DirEntry,
  FilesAsk,
  FilesOp,
  ServiceReply,
  ServiceTask,
  TaskDone,
} from './service-messages';

/** The worker's message listener, caught when its module adds it. */
type TaskListener = (event: MessageEvent<ServiceTask>) => void;

let listener: TaskListener | null = null;
const replies: ServiceReply[] = [];
let nextId = 1;

beforeAll(async () => {
  const own = globalThis.addEventListener;
  globalThis.addEventListener = ((type: string, heard: TaskListener) => {
    if (type === 'message') listener = heard;
  }) as typeof addEventListener;
  vi.stubGlobal('postMessage', (reply: ServiceReply) => replies.push(reply));
  await import('./service.worker');
  globalThis.addEventListener = own;
});
afterAll(() => vi.unstubAllGlobals());

/** Sends the worker a task (its id given here) and waits for the reply that ends it. */
async function send(task: ServiceTask): Promise<ServiceReply> {
  const id = nextId++;
  listener?.({ data: { ...task, id } } as MessageEvent<ServiceTask>);
  await vi.waitFor(() =>
    expect(replies.some((reply) => reply.id === id && reply.kind !== 'progress')).toBe(true),
  );
  return replies.find((reply) => reply.id === id && reply.kind !== 'progress') as ServiceReply;
}

/** One of the page's own file tasks. */
function filesAsk(op: FilesOp, path: string): FilesAsk {
  return { kind: 'files', id: 0, op, path, body: null, files: [], limit: 0 };
}

function chosen(path: string, text: string): ChosenFile {
  return { path, file: new File([text], path.split('/').pop() ?? '', { lastModified: 0 }) };
}

describe('the service worker before the service starts', () => {
  it('turns down requests to the service', async () => {
    const reply = await send({
      kind: 'ask',
      id: 0,
      method: 'GET',
      path: '/api/recordings',
      body: null,
    });
    expect(reply).toEqual({
      kind: 'error',
      id: reply.id,
      status: 503,
      error: 'The review service was not started',
    });
  });

  it('mounts the VODs folder as chosen files, and the page reads and lists them', async () => {
    const files = [chosen('a/b.mp4', 'video')];
    expect(await send({ kind: 'mount', id: 0, vods: files })).toMatchObject({
      kind: 'done',
      result: null,
    });
    const read = (await send(filesAsk('read', '/vods/a/b.mp4'))) as TaskDone;
    expect(await (read.result as File).text()).toBe('video');
    const list = (await send(filesAsk('list', '/vods'))) as TaskDone;
    const entries: DirEntry[] = [['a', true, 0, 0]];
    expect(list.result).toEqual(entries);
  });

  it('answers a missing file with 404, a refused write with 500, and an unmounted folder with 404', async () => {
    await send({ kind: 'mount', id: 0, vods: [chosen('a.mp4', 'video')] });
    expect(await send(filesAsk('read', '/vods/none.mp4'))).toMatchObject({
      kind: 'error',
      status: 404,
      error: 'none.mp4: not found',
    });
    expect(await send(filesAsk('write', '/vods/x.mp4'))).toMatchObject({
      kind: 'error',
      status: 500,
      error: 'read-only',
    });
    await send({ kind: 'mount', id: 0, vods: null });
    expect(await send(filesAsk('read', '/vods/a.mp4'))).toMatchObject({
      kind: 'error',
      status: 404,
    });
  });

  it('takes KovaaK files before the service has mounted them', async () => {
    expect(
      await send({ kind: 'kovaak', id: 0, files: [chosen('stats/a.csv', 'x')] }),
    ).toMatchObject({
      kind: 'done',
      result: null,
    });
  });
});
