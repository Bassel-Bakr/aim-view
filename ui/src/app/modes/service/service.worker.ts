/// <reference lib="webworker" />
// The review service (service/, built for the browser as browser-service/) in a worker of the page's own: browser mode
// answers the same API as the review server and the desktop app with it. The service's files are mounted as the
// contract says (mounts.ts): /data and /kovaak in the browser's private file system, /vods the VODs folder the user
// opened, /models the models beside the app. One request runs at a time, in the order asked.
import { moveBrowserData } from './browser-data-move';
import { DirMount, FilesMount, FsError, HttpMount, Mounts } from './mounts';
import {
  FilesAsk,
  FilesResult,
  MountAsk,
  ServiceAnswer,
  ServiceAsk,
  ServiceMethod,
  ServiceReply,
  ServiceStart,
  ServiceTask,
  VodsMount,
} from './service-messages';
import { HandleRequest, OpenFailed, ServiceModule } from './service-module';

/** The shipped area finder data the first run starts from, when /data has none of its own. */
const SHIPPED = ['area_examples.jsonl', 'area_kinds.json'];
/** Where the old browser mode remembered the VODs folder's handle (IndexedDB), as recordings-folder.ts kept it. */
const DB = 'aimview';
const STORE = 'kv';
const FOLDER_KEY = 'recordings-folder';
/** This worker's mark on the files uploads are written to first: the service clears another's at its next open. */
const SPOOL_OWNER = crypto.getRandomValues(new Uint32Array(1))[0] || 1;

const mounts = new Mounts();
let service: Promise<ServiceModule> | null = null;
/** Where the module is, and the config it opens with: kept to load it again after a trap. */
let wasmUrl = '';
let config = '';
/** The end of the queue: each task runs once the ones before it have ended. */
let tail: Promise<unknown> = Promise.resolve();
let spools = 0;

const say = (m: ServiceReply, transfer: Transferable[] = []) => postMessage(m, transfer);

/** Runs a step after every step asked before it. */
function inTurn<T>(step: () => Promise<T>): Promise<T> {
  const out = tail.then(step);
  tail = out.catch(() => undefined);
  return out;
}

/** Why a task failed, as the status the page sees: 404 for a file not there, 503 when the service is not open. */
function failure(id: number, e: unknown): void {
  const status = e instanceof FsError && e.code === 1 ? 404 : e instanceof Unready ? 503 : 500;
  say({ kind: 'error', id, status, error: e instanceof Error ? e.message : String(e) });
}

/** The service could not start: every request fails with why. */
class Unready extends Error {}

addEventListener('message', (e: MessageEvent<ServiceTask>) => {
  const task = e.data;
  if (task.kind === 'start') {
    service ??= inTurn(() => start(task));
    return;
  }
  const run = task.kind === 'ask' ? ask(task) : task.kind === 'files' ? files(task) : mount(task);
  run.catch((err: unknown) => failure(task.id, err));
});

/** The VODs folder remembered from a visit before, when the browser still lets it be read; else null. */
async function rememberedFolder(): Promise<FileSystemDirectoryHandle | null> {
  if (typeof indexedDB === 'undefined') return null;
  const db = await new Promise<IDBDatabase | null>((resolve) => {
    const req = indexedDB.open(DB);
    // a browser that never kept one gets no store made here
    req.onupgradeneeded = () => req.transaction?.abort();
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => resolve(null);
  });
  if (!db?.objectStoreNames.contains(STORE)) return null;
  const handle = await new Promise<FileSystemDirectoryHandle | null>((resolve) => {
    const req = db.transaction(STORE).objectStore(STORE).get(FOLDER_KEY);
    req.onsuccess = () => resolve((req.result as FileSystemDirectoryHandle | undefined) ?? null);
    req.onerror = () => resolve(null);
  });
  db.close();
  const leave = await handle?.queryPermission({ mode: 'read' }).catch(() => 'prompt');
  return leave === 'granted' ? handle : null;
}

/** The private file system's folder for a mount, made the first time. */
async function privateFolder(name: string): Promise<FileSystemDirectoryHandle> {
  const root = await navigator.storage.getDirectory();
  const app = await root.getDirectoryHandle('aimview', { create: true });
  return app.getDirectoryHandle(name, { create: true });
}

/** The shipped area finder data copied into /data where /data has none (the first run). */
async function fillShipped(dataUrl: string): Promise<void> {
  for (const name of SHIPPED) {
    if (await mounts.exists(`/data/${name}`)) continue;
    const res = await fetch(new URL(name, dataUrl)).catch(() => null);
    const page = (res?.headers.get('content-type') ?? '').startsWith('text/html');
    if (!res?.ok || page) continue;
    await mounts.write(`/data/${name}`, new Uint8Array(await res.arrayBuffer()));
  }
}

/** Mounts the folders, fills /data on the first run, opens the service, then moves the old browser mode's data. */
async function start(task: ServiceStart): Promise<ServiceModule> {
  try {
    mounts.set('data', new DirMount(privateFolder('data'), true));
    mounts.set('kovaak', new DirMount(privateFolder('kovaak'), true));
    mounts.set('models', new HttpMount(task.modelsUrl));
    const folder = await rememberedFolder().catch(() => null);
    if (folder) mounts.set('vods', new DirMount(Promise.resolve(folder), false));
    await fillShipped(task.dataUrl).catch((e: unknown) => console.warn('Shipped data:', e));
    wasmUrl = task.wasmUrl;
    config = JSON.stringify({
      data: '/data',
      vods: folder ? '/vods' : null,
      stats: '/kovaak/stats',
      scenarios: ['/kovaak/scenarios', '/kovaak/workshop'],
      models: '/models',
    });
    let module = await load();
    const send = async (method: ServiceMethod, path: string, body: Uint8Array) => {
      try {
        return await module.handle({ method, path }, body);
      } catch (e) {
        if (e instanceof WebAssembly.RuntimeError) module = await load();
        throw e;
      }
    };
    await moveBrowserData(send).catch((e: unknown) => console.warn('Moving browser data:', e));
    return module;
  } catch (e) {
    throw unready(e);
  }
}

/** Why the service could not start, as every request's answer then says. */
function unready(e: unknown): Unready {
  const why = e instanceof OpenFailed ? e.message : e instanceof Error ? e.message : String(e);
  return new Unready(`The review service could not start in this browser: ${why}`);
}

/** The module loaded and opened (again after a trap: a panic in Rust aborts the instance). */
async function load(): Promise<ServiceModule> {
  const module = await ServiceModule.load(wasmUrl, (op, path, arg) => mounts.host(op, path, arg));
  await module.open(config);
  return module;
}

function opened(): Promise<ServiceModule> {
  return service ?? Promise.reject(new Unready('The review service was not started'));
}

/** A request through the module; after a trap the module is loaded again for the requests that follow. */
async function handle(request: HandleRequest, body: Uint8Array): Promise<ServiceAnswer> {
  const module = await opened();
  try {
    return await module.handle(request, body);
  } catch (e) {
    if (e instanceof WebAssembly.RuntimeError) {
      service = load().catch((err: unknown) => Promise.reject(unready(err)));
    }
    throw e;
  }
}

/** A request to the service. An upload's file is written to the uploads first, and the service moves it in place. */
async function ask(task: ServiceAsk): Promise<void> {
  const answer = await inTurn(async (): Promise<ServiceAnswer> => {
    const { method, path, body } = task;
    if (!(body instanceof Blob)) return handle({ method, path }, body ?? new Uint8Array());
    if (!path.startsWith('/api/upload')) {
      return handle({ method, path }, new Uint8Array(await body.arrayBuffer()));
    }
    await opened();
    const upload = `/data/uploads/.incoming-${SPOOL_OWNER}-${spools++}.part`;
    await mounts.write(upload, body, (done, total) =>
      say({ kind: 'progress', id: task.id, done, total }),
    );
    try {
      return await handle({ method, path, upload }, new Uint8Array());
    } finally {
      // moved in place by the service; left only when the upload was turned down
      await mounts.removeFile(upload).catch(() => undefined);
    }
  });
  say({ kind: 'answer', id: task.id, answer }, [answer.body.buffer as ArrayBuffer]);
}

/** One of the page's own file tasks: a read, a listing, a write, a removal, or a copy into a folder. */
async function files(task: FilesAsk): Promise<void> {
  // the mounts are made before the service opens: the page's files are there even when the service could not start
  await opened().catch(() => undefined);
  const result: FilesResult =
    task.op === 'copy'
      ? await mounts.copyIn(task.path, task.files, inTurn, (done, total) =>
          say({ kind: 'progress', id: task.id, done, total }),
        )
      : await inTurn(async (): Promise<FilesResult> => {
          if (task.op === 'read') return mounts.file(task.path);
          if (task.op === 'list') return mounts.list(task.path, task.limit);
          if (task.op === 'write') await mounts.write(task.path, task.body ?? new Uint8Array());
          else await mounts.removeFile(task.path);
          return null;
        });
  say({ kind: 'done', id: task.id, result });
}

/** Mounts the VODs folder the user opened (or its files, or none). */
async function mount(task: MountAsk): Promise<void> {
  await inTurn(async () => {
    const vods: VodsMount | null = task.vods;
    if (!vods) mounts.set('vods', null);
    else if (Array.isArray(vods)) mounts.set('vods', new FilesMount(vods));
    else mounts.set('vods', new DirMount(Promise.resolve(vods), false));
  });
  say({ kind: 'done', id: task.id, result: null });
}
