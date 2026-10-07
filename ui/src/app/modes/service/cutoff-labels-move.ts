/// <reference lib="webworker" />
/**
 * The cut-off labels the page kept in IndexedDB before 2026-10-07 ("aimview" / "kv": the rows
 * under cutoff-rows, each crop under cutoff-crop:<file>), moved once into the review service, which
 * keeps them in its database. The old copy stays; a mark that the move is done is added beside it.
 * In: the old store. Out: the batches sent to POST /api/cutoff_labels (service.worker.ts runs it
 * after the service opens, in the queue's turn).
 */
import { CutoffCropFile, CutoffRow, labelsBatch } from './cutoff-labels';
import { ServiceSend } from './kovaak-move';

/** The old store's IndexedDB database. */
const DB = 'aimview';
/** The old store's object store in it. */
const STORE = 'kv';
/** The key of every row, in the order written. */
const ROWS_KEY = 'cutoff-rows';
/** The key prefix of a crop's .npz bytes, followed by its file name. */
const CROP_KEY = 'cutoff-crop:';
/** Added beside the old labels once they are moved, so the move runs once. */
const MOVED_KEY = 'cutoff-labels-moved';
/** The most crops in one batch: the rows go in the last. */
const BATCH_CROPS = 200;
/** A request answered. */
const OK = 200;

/** A request's result as a promise. */
function asked<T>(request: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error ?? new Error('IndexedDB failed'));
  });
}

/** The old store, when this browser has one with its object store; else null (none is made). */
async function oldStore(): Promise<IDBDatabase | null> {
  if (typeof indexedDB === 'undefined') return null;
  const open = indexedDB.open(DB);
  open.onupgradeneeded = () => open.transaction?.abort();
  const db = await asked(open).catch(() => null);
  if (db && !db.objectStoreNames.contains(STORE)) {
    db.close();
    return null;
  }
  return db;
}

/** A value of the old store, by key. */
function read<T>(db: IDBDatabase, key: string): Promise<T | undefined> {
  return asked(db.transaction(STORE).objectStore(STORE).get(key)) as Promise<T | undefined>;
}

/**
 * Sends the old labels' crops in batches, then their rows with the last batch, and marks them
 * moved. Does nothing when there are none or they were moved; a failure leaves them for the next
 * start.
 */
export async function moveCutoffLabels(send: ServiceSend): Promise<void> {
  const db = await oldStore();
  if (!db) return;
  try {
    const rows = await read<CutoffRow[]>(db, ROWS_KEY);
    if (!rows?.length || (await read<boolean>(db, MOVED_KEY))) return;
    const crops: CutoffCropFile[] = [];
    for (const file of new Set(rows.map((row) => row.file))) {
      const npz = await read<Uint8Array<ArrayBuffer>>(db, CROP_KEY + file);
      if (npz) crops.push({ file, npz });
    }
    for (let at = 0; at < crops.length || at === 0; at += BATCH_CROPS) {
      const last = at + BATCH_CROPS >= crops.length;
      const body = await labelsBatch(last ? rows : [], crops.slice(at, at + BATCH_CROPS));
      const answer = await send('POST', '/api/cutoff_labels', body);
      if (answer.status !== OK) throw new Error(`The labels were refused (${answer.status})`);
    }
    const marked = db.transaction(STORE, 'readwrite');
    marked.objectStore(STORE).put(true, MOVED_KEY);
    await new Promise<void>((resolve, reject) => {
      marked.oncomplete = () => resolve();
      marked.onerror = () => reject(marked.error ?? new Error('IndexedDB failed'));
    });
  } finally {
    db.close();
  }
}
