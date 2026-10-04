/// <reference lib="webworker" />
// The one-time move of what the old browser mode kept in IndexedDB (its own store, "aimview" / "kv") into the
// service's data folder, through the service's own routes, so the service writes every file as it always does. It
// runs in the service worker once the service is open, before the page's first request. The old store keeps its copy:
// only a mark that the move is done is added to it. Saved data is keyed by each file's fingerprint (name, size and
// date), so it is matched to the recordings in the remembered VODs folder; it waits for a visit when that folder can
// be read. KovaaK's stats files and scenarios are not moved: the old store holds no scenario files, so the user
// chooses KovaaK's folder again, which copies both.
import { AreaBox, AreaExample, AreaKind, FaintSetting, RunMarks, Tracks } from '../../api';
import { HudReading, VideoReadings } from '../wasm/review-messages';
import { ServiceAnswer, ServiceCall } from './service-messages';

const DB = 'aimview';
const STORE = 'kv';
/** Added to the old store once the move is done, so it runs once. */
const MOVED_KEY = 'moved-to-service';
/** The video types the service lists (service/src/library/recordings.rs: VIDEO_TYPES). */
const VIDEO_TYPES = ['mp4', 'mkv', 'mov', 'webm'];
/** How far below the VODs folder the old browser mode looked for videos (recordings-folder.ts: DEPTH). */
const DEPTH = 3;

/** When each model's review of a recording was saved, by model. */
type SavedModels = Record<string, number>;

/** A review as the old browser mode kept it (saved-reviews.ts: SavedReview). */
interface OldReview {
  tracks: Tracks;
  readings: VideoReadings;
  hud?: HudReading | null;
  model: string;
  found?: unknown;
}

/** A stats file as the old browser mode kept it with a recording (stats-csv.ts: StatsCsv). */
interface OldStatsCsv {
  name: string;
  text: string;
}

/** A recording's stats file in the old browser mode (local-files.ts: KeptStats). */
interface OldStatsPair {
  stats: OldStatsCsv | null;
  how: string;
}

/** What the old store held, by its keys; each missing one is empty. */
interface OldData {
  reviews: Record<string, SavedModels>;
  marks: Record<string, RunMarks>;
  faint: Record<string, FaintSetting>;
  faintSkipped: string[];
  areas: Record<string, AreaBox[]>;
  mouseLogs: Record<string, string>;
  notAim: string[];
  labelSkipped: string[];
  examples: AreaExample[];
  kinds: AreaKind[];
  statsPairs: Record<string, OldStatsPair>;
  folder: FileSystemDirectoryHandle | null;
}

/** A video below the VODs folder: its path there (the service's recording id) and its file. */
interface FolderVideo {
  path: string;
  file: File;
}

/** The old store, or null when this browser has none. */
function openOld(): Promise<IDBDatabase | null> {
  return new Promise((resolve) => {
    if (typeof indexedDB === 'undefined') return resolve(null);
    const req = indexedDB.open(DB);
    // a browser that never had the old store gets none made here
    req.onupgradeneeded = () => req.transaction?.abort();
    req.onsuccess = () =>
      resolve(
        req.result.objectStoreNames.contains(STORE) ? req.result : (req.result.close(), null),
      );
    req.onerror = () => resolve(null);
  });
}

function get<T>(db: IDBDatabase, key: string): Promise<T | undefined> {
  return new Promise((resolve) => {
    const req = db.transaction(STORE).objectStore(STORE).get(key);
    req.onsuccess = () => resolve(req.result as T | undefined);
    req.onerror = () => resolve(undefined);
  });
}

function setMoved(db: IDBDatabase): Promise<void> {
  return new Promise((resolve) => {
    const tx = db.transaction(STORE, 'readwrite');
    tx.objectStore(STORE).put(new Date().toISOString(), MOVED_KEY);
    tx.oncomplete = () => resolve();
    tx.onerror = () => resolve();
  });
}

async function readOld(db: IDBDatabase): Promise<OldData> {
  const [reviews, marks, faint, faintSkipped, areas, mouseLogs] = await Promise.all([
    get<Record<string, SavedModels>>(db, 'review-index'),
    get<Record<string, RunMarks>>(db, 'run-marks'),
    get<Record<string, FaintSetting>>(db, 'faint-cutoffs'),
    get<string[]>(db, 'faint-skipped'),
    get<Record<string, AreaBox[]>>(db, 'exclude-areas'),
    get<Record<string, string>>(db, 'mouse-logs'),
  ]);
  const [notAim, labelSkipped, examples, kinds, statsPairs, folder] = await Promise.all([
    get<string[]>(db, 'not-aim'),
    get<string[]>(db, 'label-skipped'),
    get<AreaExample[]>(db, 'area-examples'),
    get<AreaKind[]>(db, 'area-kinds'),
    get<Record<string, OldStatsPair>>(db, 'stats-pairs'),
    get<FileSystemDirectoryHandle>(db, 'recordings-folder'),
  ]);
  return {
    reviews: reviews ?? {},
    marks: marks ?? {},
    faint: faint ?? {},
    faintSkipped: faintSkipped ?? [],
    areas: areas ?? {},
    mouseLogs: mouseLogs ?? {},
    notAim: notAim ?? [],
    labelSkipped: labelSkipped ?? [],
    examples: examples ?? [],
    kinds: kinds ?? [],
    statsPairs: statsPairs ?? {},
    folder: folder ?? null,
  };
}

/** The fingerprints the old store keyed data by (saved-reviews.ts: fingerprint). */
function fingerprints(old: OldData): Set<string> {
  return new Set([
    ...Object.keys(old.reviews),
    ...Object.keys(old.marks),
    ...Object.keys(old.faint),
    ...old.faintSkipped,
    ...Object.keys(old.areas),
  ]);
}

function hasGlobal(old: OldData): boolean {
  const pairs = Object.values(old.statsPairs).some((pair) => pair.stats);
  const logs = Object.keys(old.mouseLogs).length > 0;
  const labels = old.notAim.length + old.labelSkipped.length > 0;
  return pairs || logs || labels || old.examples.length > 0 || old.kinds.length > 0;
}

/** The videos below the folder, as the old browser mode listed them (a few levels down). */
async function folderVideos(
  dir: FileSystemDirectoryHandle,
  prefix = '',
  depth = DEPTH,
): Promise<FolderVideo[]> {
  const out: FolderVideo[] = [];
  for await (const [name, entry] of dir.entries()) {
    const path = prefix ? `${prefix}/${name}` : name;
    if (entry.kind === 'directory') {
      if (depth > 1)
        out.push(...(await folderVideos(entry as FileSystemDirectoryHandle, path, depth - 1)));
    } else if (VIDEO_TYPES.includes(name.split('.').pop()?.toLowerCase() ?? '')) {
      try {
        out.push({ path, file: await (entry as FileSystemFileHandle).getFile() });
      } catch {
        // gone since it was listed
      }
    }
  }
  return out;
}

/** The service's answer to one call; a failure is logged and the move goes on. */
async function call(
  send: ServiceCall,
  method: 'GET' | 'POST',
  path: string,
  body: unknown,
): Promise<ServiceAnswer | null> {
  const bytes =
    body instanceof Uint8Array
      ? body
      : new TextEncoder().encode(
          body === null ? '' : typeof body === 'string' ? body : JSON.stringify(body),
        );
  try {
    const answer = await send(method, path, bytes);
    if (answer.status >= 400) {
      console.warn(
        `Moving browser data: ${method} ${path}: ${new TextDecoder().decode(answer.body)}`,
      );
      return null;
    }
    return answer;
  } catch (error) {
    console.warn(`Moving browser data: ${method} ${path}:`, error);
    return null;
  }
}

const withQuery = (path: string, params: Record<string, string>) =>
  `${path}?${new URLSearchParams(params).toString()}`;

/** One recording's saved data, under its id in the service. The window goes first: it starts no review then. */
async function moveRecording(
  send: ServiceCall,
  db: IDBDatabase,
  old: OldData,
  fingerprint: string,
  id: string,
): Promise<void> {
  const marks = old.marks[fingerprint];
  if (marks) await call(send, 'POST', withQuery('/api/run', { id }), marks);
  for (const model of Object.keys(old.reviews[fingerprint] ?? {})) {
    const review = await get<OldReview>(db, `review:${fingerprint}|${model}`);
    if (!review) continue;
    const body = {
      model,
      tracks: review.tracks,
      readings: review.readings,
      hud: review.hud ?? null,
      found: review.found ?? null,
    };
    await call(send, 'POST', withQuery('/api/reviewed', { id }), body);
  }
  const faint = old.faint[fingerprint];
  if (faint) await call(send, 'POST', withQuery('/api/faint', { id }), faint);
  if (old.faintSkipped.includes(fingerprint))
    await call(send, 'POST', withQuery('/api/faint_skip', { id }), null);
  const areas = old.areas[fingerprint];
  if (areas) await call(send, 'POST', withQuery('/api/exclude', { id }), areas);
}

/** The examples: the service's (the shipped ones) with each recording the browser learned replaced by its own. */
async function moveExamples(send: ServiceCall, old: OldData): Promise<void> {
  if (!old.examples.length) return;
  const shipped = await call(send, 'GET', '/api/area_examples', null);
  const own = new Set(old.examples.map((example) => example.rec));
  const kept = new TextDecoder()
    .decode(shipped?.body ?? new Uint8Array())
    .split(/\r?\n/)
    .filter((line) => line.trim() && !own.has((JSON.parse(line) as AreaExample).rec));
  const lines = [...kept, ...old.examples.map((example) => JSON.stringify(example))];
  await call(send, 'POST', '/api/area_examples', `${lines.join('\n')}\n`);
}

/**
 * Moves the old browser mode's data into the service once. `send` answers through the service (the worker's own
 * queue). Without the old store, or once moved, it does nothing; while the VODs folder cannot be read and recordings
 * have saved data, it waits for a later visit.
 */
export async function moveBrowserData(send: ServiceCall): Promise<void> {
  const db = await openOld();
  if (!db || (await get<string>(db, MOVED_KEY))) return;
  const old = await readOld(db);
  const wanted = fingerprints(old);
  if (!wanted.size && !hasGlobal(old)) return void (await setMoved(db));
  const folder = old.folder;
  const readable = !!folder && (await folder.queryPermission({ mode: 'read' })) === 'granted';
  if (wanted.size && !readable) return;
  if (folder && readable) await moveFolderData(send, db, old, folder, wanted);
  await moveKinds(send, old);
  await moveExamples(send, old);
  await moveMouseLogs(send, db, old);
  await setMoved(db);
}

/** What the recordings of the VODs folder kept: each one's data, the stats files paired with them, their labels. */
async function moveFolderData(
  send: ServiceCall,
  db: IDBDatabase,
  old: OldData,
  folder: FileSystemDirectoryHandle,
  wanted: Set<string>,
): Promise<void> {
  await call(send, 'POST', withQuery('/api/folder', { path: '/vods' }), null);
  const videos = await folderVideos(folder);
  const byPrint = new Map(
    videos.map((video) => [
      `${video.file.name}|${video.file.size}|${video.file.lastModified}`,
      video.path,
    ]),
  );
  for (const fingerprint of wanted) {
    const id = byPrint.get(fingerprint);
    if (id) await moveRecording(send, db, old, fingerprint, id);
  }
  await moveStatsPairs(send, old);
  for (const id of old.notAim)
    await call(send, 'POST', withQuery('/api/not_aim', { id, on: '1' }), null);
  for (const id of old.labelSkipped)
    await call(send, 'POST', withQuery('/api/label_skip', { id }), null);
}

/** The stats files the user paired with the folder's recordings, uploaded beside them. */
async function moveStatsPairs(send: ServiceCall, old: OldData): Promise<void> {
  for (const [key, pair] of Object.entries(old.statsPairs)) {
    const id = key.startsWith('folder:') ? key.slice('folder:'.length) : null;
    if (!id || !pair.stats || !['picked', 'upload', 'beside'].includes(pair.how)) continue;
    await call(
      send,
      'POST',
      withQuery('/api/upload', { name: pair.stats.name, id }),
      pair.stats.text,
    );
  }
}

/** The area types: each under its id, or as a new type when the service turns the id down. */
async function moveKinds(send: ServiceCall, old: OldData): Promise<void> {
  for (const kind of old.kinds) {
    const edit = { id: kind.id, name: kind.name, about: kind.about };
    if (!(await call(send, 'POST', '/api/area_kinds', edit)))
      await call(send, 'POST', '/api/area_kinds', { ...edit, id: null });
  }
}

/** The raw mouse logs, each under its name. */
async function moveMouseLogs(send: ServiceCall, db: IDBDatabase, old: OldData): Promise<void> {
  for (const [fingerprint, name] of Object.entries(old.mouseLogs)) {
    const log = await get<ArrayBuffer>(db, `mouse-log|${fingerprint}`);
    if (log) await call(send, 'POST', withQuery('/api/mouse_log', { name }), new Uint8Array(log));
  }
}
